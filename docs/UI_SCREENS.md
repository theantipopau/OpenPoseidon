# UI Screens

Status of the Proteus design system across every full-window screen. Source: the Step 0 audit in `docs/PROTEUS_UI_SUBSCREEN_PROMPT.md`.

The engine's UI is a `Display` tree built on `ControlsContainer` (`engine/Poseidon/UI/Controls/UIControls.hpp`). Most displays load their layout from a retail resource class (`Load("RscDisplay...")`). Those layouts are data, not code, so the Proteus pass restyles them at load time and repositions the controls that need it.

## Sub-screens (the seven named in the spec)

| Screen Name | Display class | File(s) | Layout source | Status |
| --- | --- | --- | --- | --- |
| Main Menu | `DisplayMain` | `engine/Poseidon/UI/OptionsUIApp.cpp` | Code (Proteus layout) | Done (overhaul commits `679e7b0`–`e2c3721`) |
| Campaign Select | `DisplayCampaignLoad` | `engine/Poseidon/UI/OptionsUIImpl.cpp` (`RscDisplayCampaignLoad`) | Resource | Pending |
| Single Missions | `DisplaySingleMission` | `engine/Poseidon/UI/OptionsUIImpl.cpp` (`RscDisplaySingleMission`) | Resource | Pending |
| Multiplayer | `DisplayMultiplayer` | `engine/Poseidon/UI/DisplayUIMultiplayer.cpp` (`RscDisplayMultiplayer`) | Resource | Pending |
| Local Maps | `LocalMapsDisplay` | `engine/Poseidon/UI/LocalMapsDisplay.cpp` / `.hpp` | Code (`AddControl`) | Removed from the main menu. Crashes when opened: `ScanLocalVehicles` throws |
| Mission Editor (launcher) | `DisplaySelectIsland` | `engine/Poseidon/UI/DisplayUI.cpp` / `DisplayUI.hpp` | Resource | Pending. Editor UI itself is out of scope |
| Mods | `DisplayMods` | `engine/Poseidon/UI/OptionsUIApp.cpp` (`RscDisplayMods`) | Resource | Removed from the main menu. Not reachable |
| Options | `DisplayOptions` | `engine/Poseidon/UI/OptionsUIImpl.cpp` (`RscDisplayOptions`); pages in `engine/Poseidon/UI/Options/` | Resource + pages | Main page done (`997c441`). Video, Audio, Controls and Difficulty pages still use the retail look |

## Other full-window displays found

These are not in the spec's list. Flagged for a later pass.

| Screen Name | Display class | File(s) | Notes |
| --- | --- | --- | --- |
| Campaign intro / load / template | `DisplayCampaignIntro`, `DisplayTemplateLoad`, `DisplayTemplateSave` | `DisplayUI.hpp`, `OptionsUIImpl.cpp` | Child screens of the campaign and templates flows |
| Login / new user / server | `DisplayLogin`, `DisplayNewUser`, `DisplayServer` | `DisplayUI.hpp` | Multiplayer child screens |
| Client / IP / port / password | `DisplayClient`, `DisplayIPAddress`, `DisplayPort`, `DisplayPassword` | `DisplayUI.hpp` | Multiplayer connect flow |
| Multiplayer setup / players | `DisplayMultiplayerSetup`, `DisplayMPPlayers`, `DisplayJoinRequirements` | `DisplayUI.hpp` | Multiplayer child screens |
| Mod download | `DisplayModDownload` | `DisplayUI.hpp` | Mods child screen |
| Custom arcade | `DisplayCustomArcade` | `DisplayUI.hpp` | Reached from the main menu's old custom entry |
| Credits / intro / outro / cutscene | `DisplayCredits`, `DisplayIntro`, `DisplayOutro`, `DisplayCutscene` | `DisplayUI.hpp` | Cinematic screens; not restyled |
| Remote missions / wizard | `DisplayRemoteMissions`, `DisplayWizardMap`, `DisplayWizardTemplate` | `DisplayUI.hpp` | Wizard flows |
| Map / notebook / debriefing | `DisplayMap`, `DisplayNotebook`, `DisplayDebriefing`, `DisplayMissionEnd` | `engine/Poseidon/UI/Map/UIMap.hpp` | In-game screens; part of the HUD boundary |
| In-game HUD | `DisplayGame`, `DisplaySave`, `DisplayInventory`, `DisplayMission`, `InGameUI*` | `DisplayUI.hpp`, `InGame/`, `Inventory/` | Excluded: the spec says not to restyle the in-game HUD |
| Dev tools / arcade editing | `DisplayDebug`, `DisplayArcade*`, `DisplayMapEditor` | `DisplayUI.hpp`, `Map/UIMap.hpp` | Developer tooling |
| Message box | `MsgBox` | `DisplayUI.hpp` | Used by every screen for confirmations |

## Notes

- The Mission Editor entry opens a launcher (`DisplaySelectIsland`). The editor UI behind it is a separate task and is not restyled here.
- The in-game HUD is out of scope, per the spec.
- `ControlsContainer::OnCreateCtrl` is the factory for every control (`engine/Poseidon/UI/Map/UIContainers.cpp`). The Proteus button is created there for screens that opt in.
