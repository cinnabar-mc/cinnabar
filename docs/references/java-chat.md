# Built-in Java-look chat

The owner chose Java chat geometry for the built-in look. This is a styling
exception, not a vanilla parity claim. `assets/java-hud/ui/chat_screen.json`
changes presentation only; the existing chat editor and input host still send,
paste, navigate sent history, and complete commands. The owner selected built-in
chat over server layouts. Its screen, suggestions and HUD history retain their
geometry and style; other server HUD widgets still use the pack stack.

The focused history ends at the same bottom offset as HUD chat. The input bar
and history share the pack's chat width. Focused history does not fade or give
way to autocomplete; suggestions overlay it immediately above the input.

Video settings expose **Chat Position** with **Bottom** (the existing default)
and **Top**. The saved preference moves HUD notifications and focused history.
Top notifications reserve the native position and days-played label heights.
The editor, suggestions, server HUD widgets and built-in chat priority retain
their existing behavior. This placement choice is a Cinnabar presentation
extension, not a new vanilla parity claim.

**Chat Input**, next to Chat Position, chooses how the open chat looks. **Java**
(the default) keeps the Java-look input line, suggestions and focused history
above. **Bedrock** draws the pinned pack's own `chat.chat_screen_content`
instead: the full-screen backdrop, the Exit header, history anchored above the
full-width input bar, and the settings and send buttons. In this mode the Java
line and suggestion backgrounds are hidden, history ignores Top placement and
gives way to command completions as vanilla's does. History text keeps
vanilla's flush-left position instead of Java's 4 px inset. Completions rest on
the input bar, and a list longer than the completion area fills it from the
top, as a live 1.26.60.4 client showed on 2026-11-05. The built-in pack keeps
the completion grid at its rows' natural height, at the top of a panel that fits
it up to that area and rests on the input bar; the panel clips rows past its
bottom edge. Vanilla's own grid fills the area,
and the native rule that places its rows at the bottom has not been identified.
A server pack that restyles the chat screen without this override therefore
lists completions from the top. Vanilla also ends a truncated list with a `...`
row, which is not drawn yet. Top or Bottom still places
HUD notifications. The Java-look pack keeps screen animations off in both modes,
so vanilla's slide-in transition is not drawn. A live 1.26.60.4 client showed the
same arrangement on 2026-11-05.

**Bedrock Hotbar Position**, on by default, lifts the Java-look hotbar to
vanilla desktop's height: the pinned pack's hotbar ends 1.5 GUI px above the
bottom edge. Status rows, XP bar, offhand, item name, HUD chat and focused
history rise with it, so their Java spacing above the hotbar stays the same.
Turning it off keeps Java's flush hotbar.

## Vanilla rules

| Rule | Behaviour |
| --- | --- |
| Chat controller | Own clipboard paste and binding registration. Current binding implementation details remain unverified. |
| Sending | Send the message or dismiss blank input. |
| Sent history | Select previously sent messages. |

- Vanilla `v1.26.50.4/full/resource_pack/ui/chat_screen.json:308`: the
  `messages_factory` and scrolling panel, including jump-to-bottom on update.
  Lines 339 and 523 define the editor content binding and autocomplete rows;
  line 948 defines the chat screen and cancel mappings. `ui/ui_common.json:4427`
  defines the scroll view, and `ui/hud_screen.json:3574` defines HUD input policy.
  `texts/en_US.lang:584` supplies the existing chat title localization.

The offline snapshots composite the real carrier's UI over a plain backdrop.
They verify overlay geometry and HUD layering, not live world rendering or
target-platform visual parity.
