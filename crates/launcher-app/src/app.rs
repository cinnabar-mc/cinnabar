//! The launcher window's event loop: input, starting the client and repaint pacing.

use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bootstrap_ui::{
    OVERLAY_INTERVAL, PANORAMA_TINT, Preferences,
    canvas::{Canvas, Rect, Text},
    gpu,
    input::{self, Command, Input, Source, pointer::Pointer},
};
use launcher::install_layout::InstallLayout;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::ModifiersState,
    window::{Window, WindowId},
};

use crate::{
    EXIT_UNAVAILABLE, WINDOW_SIZE, play,
    screen::{Action, Effect, Launch, Overview, Screen},
    view,
};

/// How long the launcher shows that the client is starting before it closes.
const LAUNCH_LINGER: Duration = Duration::from_millis(1500);

pub(crate) struct LauncherApp {
    layout: InstallLayout,
    overview: Overview,
    text: Text,
    /// Uploaded once the GPU exists.
    faces: Option<render_model::PanoramaFaces>,
    art: view::Art,
    window: Option<Arc<Window>>,
    gpu: Option<gpu::Gpu>,
    screen: Screen,
    hits: Vec<(Action, Rect)>,
    pointer: Pointer,
    hovered: Option<Action>,
    input: Input<Action>,
    controllers: Option<gilrs::Gilrs>,
    preferences: Preferences,
    modifiers: ModifiersState,
    overlay_dirty: bool,
    overlay_at: Option<Instant>,
    launched_at: Option<Instant>,
    epoch: Instant,
    exit: Option<i32>,
}

impl LauncherApp {
    pub(crate) fn new(
        layout: InstallLayout,
        text: Text,
        faces: render_model::PanoramaFaces,
    ) -> Self {
        let preferences = Preferences::load(&layout);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        let overview = Overview::read(
            &layout,
            preferences.settings.update_preferences().channel,
            now,
        );
        let avatar = overview
            .account
            .as_ref()
            .and_then(|account| account.picture.as_deref())
            .and_then(view::Art::load_avatar);
        let art = view::Art::new(bootstrap_ui::decode_logo(), avatar);
        let screen = Screen::new(overview.servers.len());
        let mut input = Input::default();
        input.reset(&screen);
        Self {
            layout,
            overview,
            text,
            faces: Some(faces),
            art,
            window: None,
            gpu: None,
            screen,
            hits: Vec::new(),
            pointer: Pointer::default(),
            hovered: None,
            input,
            controllers: bootstrap_ui::controllers(),
            preferences,
            modifiers: ModifiersState::default(),
            overlay_dirty: true,
            overlay_at: None,
            launched_at: None,
            epoch: Instant::now(),
            exit: None,
        }
    }

    /// The exit code chosen by the flow, if any.
    pub(crate) const fn exit_code(&self) -> Option<i32> {
        self.exit
    }

    fn show(&mut self, next: Screen) {
        self.input.update_screen(&self.screen, &next);
        self.hovered = None;
        self.screen = next;
        self.overlay_dirty = true;
    }

    fn act(&mut self, action: Action) {
        let (next, effect) = self.screen.on_action(action, &self.overview);
        if let Some(next) = next {
            self.show(next);
        }
        match effect {
            Effect::None => {}
            Effect::Launch(address) => {
                let result = play::start(&self.layout, address.as_deref());
                let next = self.screen.launched(result, self.overview.servers.len());
                if next.launch == Launch::Starting {
                    self.launched_at = Some(Instant::now());
                }
                self.show(next);
            }
            Effect::OpenFolder => {
                launcher_host::desktop::open_folder(self.layout.user_data_root.clone());
            }
        }
    }

    fn finish(&mut self, code: i32, event_loop: &ActiveEventLoop) {
        self.exit = Some(code);
        event_loop.exit();
    }

    fn hit(&self) -> Option<Action> {
        let (x, y) = self.pointer.hit_position()?;
        self.hits
            .iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(action, _)| *action)
    }

    fn repaint_overlay(&mut self) {
        let (Some(window), Some(gpu)) = (&self.window, &mut self.gpu) else {
            return;
        };
        let size = window.inner_size();
        let mut canvas = Canvas::new(size.width.max(1), size.height.max(1));
        self.hits = view::draw(
            &mut canvas,
            &mut self.text,
            &mut self.art,
            &self.screen,
            &self.overview,
            &view::Style {
                rem: self.preferences.rem(size),
                hovered: self.hovered,
                focused: self.input.focused.filter(|_| self.input.focus_visible),
                pressed: self.input.pressed(self.hovered),
            },
        );
        gpu.set_overlay(canvas.width, canvas.height, &canvas.pixels);
        let hovered = self.hit();
        self.overlay_dirty = hovered != self.hovered;
        self.hovered = hovered;
        self.overlay_at = Some(Instant::now());
    }

    fn command(&mut self, source: Source, command: Command) {
        let hit = (source == Source::Pointer)
            .then_some(self.hovered)
            .flatten();
        let (action, changed) = self.input.apply(&self.screen, source, command, hit);
        self.overlay_dirty |= changed;
        if let Some(action) = action {
            self.act(action);
        }
    }
}

impl ApplicationHandler for LauncherApp {
    #[expect(
        clippy::print_stderr,
        reason = "the launcher installs no log subscriber; stderr is its only diagnostic channel"
    )]
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = bootstrap_ui::window::attributes(WINDOW_SIZE);
        let Ok(window) = event_loop.create_window(attributes) else {
            self.finish(EXIT_UNAVAILABLE, event_loop);
            return;
        };
        let window = Arc::new(window);
        let Some(faces) = self.faces.take() else {
            return;
        };
        match gpu::Gpu::new(Arc::clone(&window), &faces) {
            Ok(gpu) => self.gpu = Some(gpu),
            Err(error) => {
                eprintln!("launcher window unavailable: {error:#}");
                self.finish(EXIT_UNAVAILABLE, event_loop);
                return;
            }
        }
        self.window = Some(window);
    }

    #[expect(
        clippy::print_stderr,
        reason = "the launcher installs no log subscriber; stderr is its only diagnostic channel"
    )]
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => self.finish(0, event_loop),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                self.overlay_dirty = true;
            }
            WindowEvent::ScaleFactorChanged { .. } => self.overlay_dirty = true,
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer.position = Some((position.x as f32, position.y as f32));
                let hovered = self.hit();
                if hovered != self.hovered {
                    self.hovered = hovered;
                    self.input.focus_visible = false;
                    self.overlay_dirty = true;
                }
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer.position = None;
                self.hovered = None;
                self.overlay_dirty = true;
            }
            WindowEvent::Focused(active) => {
                self.pointer.focus(active);
                self.hovered = self.hit();
                if !active {
                    self.input.blur();
                    self.modifiers = ModifiersState::default();
                }
                self.overlay_dirty = true;
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let command = if state == ElementState::Pressed {
                    Command::Press
                } else {
                    Command::Release
                };
                self.command(Source::Pointer, command);
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state,
                        logical_key,
                        repeat: false,
                        ..
                    },
                is_synthetic,
                ..
            } => {
                let (action, changed) = self.input.keyboard(
                    &self.screen,
                    &logical_key,
                    state,
                    self.modifiers.shift_key(),
                    is_synthetic,
                );
                self.overlay_dirty |= changed;
                if let Some(action) = action {
                    self.act(action);
                }
            }
            WindowEvent::RedrawRequested => {
                let (Some(window), Some(gpu)) = (&self.window, &mut self.gpu) else {
                    return;
                };
                let size = window.inner_size();
                let aspect = size.width.max(1) as f32 / size.height.max(1) as f32;
                let seconds = self.epoch.elapsed().as_secs_f32();
                if let Err(error) = gpu.draw(&render_model::panorama::launcher_view(
                    seconds,
                    aspect,
                    PANORAMA_TINT,
                )) {
                    eprintln!("launcher display failed: {error}");
                    self.finish(EXIT_UNAVAILABLE, event_loop);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        while let Some(event) = self.controllers.as_mut().and_then(gilrs::Gilrs::next_event) {
            if !self.pointer.active {
                continue;
            }
            if let Some(command) = input::controller(event.event, &self.preferences.settings) {
                self.command(Source::Controller, command);
            }
        }
        let now = Instant::now();
        if self
            .launched_at
            .is_some_and(|at| now.duration_since(at) >= LAUNCH_LINGER)
        {
            self.finish(0, event_loop);
            return;
        }
        let due = self
            .overlay_at
            .is_none_or(|at| now.duration_since(at) >= OVERLAY_INTERVAL);
        if self.overlay_dirty && due {
            self.repaint_overlay();
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}
