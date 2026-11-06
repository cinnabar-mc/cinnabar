//! Proximity voice chat: owns the `client-voice` session, feeds it the player's pose and runs
//! the `/voz` chat commands. The microphone opens only when a command asks for it.

use std::{net::Ipv4Addr, sync::Arc};

use bevy::prelude::{App, ButtonInput, KeyCode, Real, Res, ResMut, Resource, Time, Update};
use client_presentation::local_player::LocalViewPose;
use client_shell::ui_resources::UiRuntimeResource;
use client_voice::{
    device::{CpalIo, DeviceReport},
    engine::{Status, VoiceHandle},
    proto::Position,
};

use crate::runtime::world::ClientWorld;

/// Hold to talk while push-to-talk is on and the chat is closed.
const PUSH_TO_TALK_KEY: KeyCode = KeyCode::KeyV;

const HELP: [&str; 7] = [
    "/voz crear [IP]  abre una sala y te da un código (IP: tu IP pública, si juegas por internet)",
    "/voz unir CÓDIGO  entra a la sala de un amigo",
    "/voz salir  /voz estado",
    "/voz mute  /voz hablar  silencia o activa tu micrófono",
    "/voz sordo  /voz oir  deja de oír a todos o vuelve a oír",
    "/voz ptt on|off  pulsar para hablar con la tecla V",
    "/voz volumen 0-200  volumen general en porcentaje",
];

/// Registers the voice resource and its systems.
pub(crate) fn configure(app: &mut App) {
    app.init_resource::<VoiceChat>().add_systems(
        Update,
        (run_voice_commands, feed_voice_pose, drive_push_to_talk),
    );
}

#[derive(Resource, Default)]
pub(crate) struct VoiceChat {
    handle: Option<VoiceHandle>,
    announced_status: Option<Status>,
    announced_code: Option<String>,
    talking: bool,
}

#[derive(Debug, PartialEq)]
enum Command {
    Help,
    Host(Option<Ipv4Addr>),
    Join(String),
    Leave,
    Status,
    Muted(bool),
    Deafened(bool),
    PushToTalk(bool),
    Volume(f32),
}

/// Reads one `/voz` line. The error is a message for the player.
fn parse(line: &str) -> Result<Command, String> {
    let mut words = line.split_whitespace();
    words.next(); // "/voz" or "/voice"
    let sub = words.next().unwrap_or("ayuda").to_lowercase();
    let rest: Vec<&str> = words.collect();
    let first = rest.first().copied();
    Ok(match sub.as_str() {
        "ayuda" | "help" | "?" => Command::Help,
        "crear" | "host" | "create" => match first {
            None => Command::Host(None),
            Some(text) => Command::Host(Some(
                text.parse().map_err(|_| format!("'{text}' no es una dirección IPv4 válida"))?,
            )),
        },
        "unir" | "entrar" | "join" => {
            if rest.is_empty() {
                return Err("Falta el código: /voz unir XXXX-XXXX-XXXX-XXXX-XXXX-XXXX".into());
            }
            Command::Join(rest.concat())
        }
        "salir" | "leave" => Command::Leave,
        "estado" | "status" => Command::Status,
        "mute" | "silenciar" => Command::Muted(true),
        "hablar" | "unmute" => Command::Muted(false),
        "sordo" | "deafen" => Command::Deafened(true),
        "oir" | "oír" | "undeafen" => Command::Deafened(false),
        "ptt" => match first {
            Some("on" | "si" | "sí") => Command::PushToTalk(true),
            Some("off" | "no") => Command::PushToTalk(false),
            _ => return Err("Usa: /voz ptt on  o  /voz ptt off".into()),
        },
        "volumen" | "volume" => {
            let percent: f32 = first
                .and_then(|text| text.trim_end_matches('%').parse().ok())
                .ok_or("Usa: /voz volumen 0-200")?;
            Command::Volume(percent.clamp(0.0, 200.0) / 100.0)
        }
        other => return Err(format!("Comando desconocido '{other}'. Escribe /voz ayuda")),
    })
}

/// The player id voice uses: the XUID, or a stable hash of the name when there is none.
fn player_id(name: &str, xuid: &str) -> u64 {
    xuid.parse().unwrap_or_else(|_| {
        name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    })
}

fn describe_devices(report: &DeviceReport) -> Vec<String> {
    let mut lines = vec![
        report
            .microphone
            .as_ref()
            .map_or_else(|| "Sin micrófono".to_owned(), |name| format!("Micrófono: {name}")),
        report
            .speaker
            .as_ref()
            .map_or_else(|| "Sin altavoces".to_owned(), |name| format!("Altavoces: {name}")),
    ];
    lines.extend(report.problems.iter().cloned());
    lines
}

impl VoiceChat {
    /// Opens the audio devices and starts the session the first time it is needed.
    fn ensure_started(&mut self, name: &str, xuid: &str, lines: &mut Vec<String>) -> Option<VoiceHandle> {
        if self.handle.is_none() {
            match CpalIo::open() {
                Ok(io) => {
                    lines.extend(describe_devices(io.report()));
                    self.handle = Some(VoiceHandle::spawn(io, player_id(name, xuid)));
                }
                Err(why) => {
                    lines.push(format!("No se pudo abrir el audio: {why}"));
                    return None;
                }
            }
        }
        self.handle.clone()
    }

    /// Runs one `/voz` line and returns what to tell the player.
    fn execute(&mut self, line: &str, name: &str, xuid: &str) -> Vec<String> {
        let command = match parse(line) {
            Ok(Command::Help) => return HELP.iter().map(|&text| text.to_owned()).collect(),
            Ok(command) => command,
            Err(message) => return vec![message],
        };
        let mut lines = Vec::new();
        let Some(handle) = self.ensure_started(name, xuid, &mut lines) else {
            return lines;
        };
        match command {
            Command::Help => {}
            Command::Host(address) => {
                handle.host_room(address);
                lines.push("Creando sala…".into());
            }
            Command::Join(code) => match handle.join_room(&code) {
                Ok(()) => lines.push("Conectando a la sala…".into()),
                Err(why) => lines.push(format!("Código inválido: {why}")),
            },
            Command::Leave => {
                handle.leave();
                lines.push("Saliste de la sala.".into());
            }
            Command::Status => lines.extend(status_lines(&handle)),
            Command::Muted(on) => {
                handle.set_muted(on);
                lines.push(if on { "Micrófono silenciado." } else { "Micrófono activo." }.into());
            }
            Command::Deafened(on) => {
                handle.set_deafened(on);
                lines.push(if on { "Ya no oyes a nadie." } else { "Vuelves a oír." }.into());
            }
            Command::PushToTalk(on) => {
                handle.set_push_to_talk(on);
                lines.push(
                    if on { "Pulsar para hablar: mantén V." } else { "Micrófono abierto con puerta de ruido." }
                        .into(),
                );
            }
            Command::Volume(volume) => {
                handle.set_master_volume(volume);
                lines.push(format!("Volumen general: {:.0}%", volume * 100.0));
            }
        }
        lines
    }

    /// Status changes since the last call, so the player sees connections succeed or fail.
    fn announcements(&mut self) -> Vec<String> {
        let Some(handle) = &self.handle else { return Vec::new() };
        let snapshot = handle.snapshot();
        let mut lines = Vec::new();
        if self.announced_status.as_ref() != Some(&snapshot.status) {
            match &snapshot.status {
                Status::Off => {}
                Status::Connecting => lines.push("Conectando…".to_owned()),
                Status::Connected if snapshot.hosting => {
                    lines.push("Sala abierta. Tus amigos ya pueden unirse.".to_owned());
                }
                Status::Connected => lines.push("Conectado a la sala.".to_owned()),
                Status::Failed(why) => lines.push(format!("Error: {why}")),
            }
            self.announced_status = Some(snapshot.status.clone());
        }
        if self.announced_code != snapshot.room_code {
            if let Some(code) = &snapshot.room_code {
                lines.push(format!("Código de sala: {code}"));
                lines.push(
                    "Pásalo a tus amigos. Abre el puerto UDP 24454 en tu router, o usa /voz crear TU.IP.PÚBLICA."
                        .to_owned(),
                );
            }
            self.announced_code = snapshot.room_code;
        }
        lines
    }
}

fn status_lines(handle: &VoiceHandle) -> Vec<String> {
    let snapshot = handle.snapshot();
    let state = match &snapshot.status {
        Status::Off => "sin sala".to_owned(),
        Status::Connecting => "conectando".to_owned(),
        Status::Connected if snapshot.hosting => "anfitrión".to_owned(),
        Status::Connected => "conectado".to_owned(),
        Status::Failed(why) => format!("error: {why}"),
    };
    let mut lines = vec![format!(
        "Estado: {state}. Micrófono {} | audio {} | volumen {:.0}%",
        if snapshot.muted { "silenciado" } else { "activo" },
        if snapshot.deafened { "apagado" } else { "activo" },
        snapshot.master_volume * 100.0
    )];
    if let Some(code) = &snapshot.room_code {
        lines.push(format!("Código de sala: {code}"));
    }
    if !snapshot.speakers.is_empty() {
        lines.push(format!("Hablando ahora: {}", snapshot.speakers.len()));
    }
    lines
}

fn run_voice_commands(
    mut runtime: ResMut<UiRuntimeResource>,
    mut voice: ResMut<VoiceChat>,
    time: Res<Time<Real>>,
) {
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut replies = Vec::new();
    for command in runtime.take_local_commands() {
        let (name, xuid) = {
            let (name, xuid) = runtime.chat_identity();
            (name.to_owned(), xuid.to_owned())
        };
        replies.extend(voice.execute(&command, &name, &xuid));
    }
    replies.extend(voice.announcements());
    for line in replies {
        runtime.push_local_chat_line(Arc::from(format!("[Voz] {line}")), now_millis);
    }
}

/// Gives the session where the player stands and faces, for distance and stereo placement.
fn feed_voice_pose(voice: Res<VoiceChat>, view: Res<LocalViewPose>, world: Res<ClientWorld>) {
    let Some(handle) = &voice.handle else { return };
    let feet = view.feet_translation();
    let dimension = world.stream.as_ref().map_or(0, |stream| stream.current_dimension());
    // Bevy yaw runs counter-clockwise from -Z; voice uses Bedrock yaw, 0 facing +Z.
    let (yaw, _pitch) = view.look_angles();
    let bedrock_yaw = (180.0 - yaw.to_degrees()).rem_euclid(360.0);
    let dimension = u8::try_from(dimension).unwrap_or(0);
    handle.set_local_player(Position::new(dimension, feet.x, feet.y, feet.z), bedrock_yaw);
}

fn drive_push_to_talk(
    mut voice: ResMut<VoiceChat>,
    keys: Res<ButtonInput<KeyCode>>,
    runtime: Res<UiRuntimeResource>,
) {
    let Some(handle) = voice.handle.clone() else { return };
    let held = keys.pressed(PUSH_TO_TALK_KEY) && !runtime.chat_focused();
    if held != voice.talking {
        voice.talking = held;
        handle.set_talking(held);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_parse_in_spanish_and_english() {
        assert_eq!(parse("/voz"), Ok(Command::Help));
        assert_eq!(parse("/voice crear"), Ok(Command::Host(None)));
        assert_eq!(
            parse("/voz crear 203.0.113.7"),
            Ok(Command::Host(Some(Ipv4Addr::new(203, 0, 113, 7))))
        );
        assert_eq!(parse("/voz unir abcd-efgh ijkl"), Ok(Command::Join("abcd-efghijkl".into())));
        assert_eq!(parse("/voz mute"), Ok(Command::Muted(true)));
        assert_eq!(parse("/voz oir"), Ok(Command::Deafened(false)));
        assert_eq!(parse("/voz ptt on"), Ok(Command::PushToTalk(true)));
        assert_eq!(parse("/voz volumen 50%"), Ok(Command::Volume(0.5)));
        assert_eq!(parse("/voz volumen 900"), Ok(Command::Volume(2.0)));
    }

    #[test]
    fn bad_input_explains_itself() {
        assert!(parse("/voz unir").is_err());
        assert!(parse("/voz crear no-es-ip").is_err());
        assert!(parse("/voz ptt").is_err());
        assert!(parse("/voz volumen").is_err());
        assert!(parse("/voz baila").unwrap_err().contains("baila"));
    }

    #[test]
    fn player_id_prefers_the_xuid() {
        assert_eq!(player_id("Steve", "2535400000000001"), 2_535_400_000_000_001);
        assert_eq!(player_id("Steve", ""), player_id("Steve", "no es numero"));
        assert_ne!(player_id("Steve", ""), player_id("Alex", ""));
    }

    #[test]
    fn help_needs_no_audio_devices() {
        let mut chat = VoiceChat::default();
        let lines = chat.execute("/voz ayuda", "Steve", "1");
        assert_eq!(lines.len(), HELP.len());
        assert!(chat.handle.is_none());
    }
}
