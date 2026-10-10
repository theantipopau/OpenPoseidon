# Proteus — Main Menu UI Overhaul
> **For:** Claude Code / AI Coding Agent  
> **Task:** Full visual overhaul of the main menu screen  
> **Reference screenshot:** Current state shows a hard 50/50 split layout with flat grey button panel — this needs to be completely replaced  
> **Priority:** High — this is the face of the project  

---

## 🎯 Goal

Replace the current main menu layout with a full-bleed cinematic design:
- Background image covers the **entire window**
- Left sidebar is a **semi-transparent glass panel** sitting *on top* of the background
- Buttons are clean, minimal, military-styled with a cyan accent hover state
- Logo is properly positioned with breathing room
- Window title updated to `Proteus`
- Font upgraded throughout

---

## 📋 Step 0 — Read Before Touching

Before writing any code:

1. Find the main menu rendering code. Search for any file referencing:
   - `"CAMPAIGN"` string literal
   - `main_menu`, `MainMenu`, `main menu`
   - `imgui::begin` or equivalent UI panel calls
   - Background image / texture loading for the menu screen

2. Identify:
   - What UI framework is in use (ImGui is likely — confirm version and whether `imgui-rs` Rust bindings or C++ ImGui is used)
   - Where the background image is loaded and rendered
   - Where button layout/sizing is defined
   - Where the window title string is set
   - Where fonts are loaded

3. Report all file paths found before making any changes.

---

## 🪟 Fix 1 — Window Title

Find where the application window title is set (likely in `engine/` or `apps/`).

Change:
```
"Poseidon [WGPU]"   →   "Proteus"
```

If the renderer name is appended dynamically (e.g., `format!("Poseidon [{}]", renderer)`), change the base name only:
```rust
format!("Proteus [{}]", renderer)
// or simply:
"Proteus"
```

Commit: `fix(ui): rename window title to Proteus`

---

## 🖼️ Fix 2 — Full-Bleed Background

The background image must cover **the entire window**, not just the left half.

**Current behaviour:** Background renders in left ~50% of window. Right half is a solid dark grey panel.

**Target behaviour:** Background image stretches/fills 100% of window width and height. The sidebar panel is then drawn *on top* of it.

Find the background rendering call. It will be something like:
```cpp
// C++ ImGui example
ImGui::Image(background_texture, ImVec2(half_width, window_height));
// or in Rust imgui-rs:
ui.image(background_texture_id, [half_width, window_height]);
```

Change to use the full window size:
```cpp
ImGui::Image(background_texture, ImVec2(full_width, full_height));
// Rust:
ui.image(background_texture_id, [full_width, full_height]);
```

If the background is rendered as a separate ImGui window/panel, ensure it has:
- Position: `[0.0, 0.0]`
- Size: `[window_width, window_height]`
- Flags: `NO_DECORATION | NO_MOVE | NO_BRING_TO_DISPLAY_FRONT | NO_INPUT`
- Rendered **first** so everything else draws on top

Also add a **dark vignette gradient** over the background for readability. If a full shader pass isn't available, approximate with a dark semi-transparent ImGui rectangle covering the full window at low alpha:
```cpp
// Draw after background, before sidebar
ImDrawList* draw_list = ImGui::GetBackgroundDrawList();
draw_list->AddRectFilledMultiColor(
    ImVec2(0, 0), ImVec2(full_width, full_height),
    IM_COL32(0, 0, 0, 120),  // top-left: dark
    IM_COL32(0, 0, 0, 0),    // top-right: transparent
    IM_COL32(0, 0, 0, 0),    // bottom-right: transparent
    IM_COL32(0, 0, 0, 80)    // bottom-left: slight dark
);
```

Commit: `feat(ui): full-bleed background across entire menu window`

---

## 🗂️ Fix 3 — Sidebar Panel (Glass Effect)

Replace the current solid grey right-side panel with a semi-transparent left-side sidebar.

**Target spec:**
- **Position:** Left edge, full height
- **Width:** 32% of window width (minimum 260px, maximum 380px)
- **Background colour:** `rgba(8, 14, 20, 0.82)` — very dark navy, mostly opaque
- **Left border:** 3px solid `rgba(0, 188, 212, 0.9)` — cyan accent (draw as a filled rect strip on the left edge)
- **No title bar, no scrollbar, no resize, no move**
- **Padding:** 24px horizontal, 20px vertical

ImGui implementation:
```cpp
// Rust imgui-rs style — adjust for C++ as needed
let sidebar_width = (window_width * 0.32).max(260.0).min(380.0);

ImGui::SetNextWindowPos(ImVec2(0.0, 0.0));
ImGui::SetNextWindowSize(ImVec2(sidebar_width, window_height));
ImGui::SetNextWindowBgAlpha(0.82);

ImGui::PushStyleColor(ImGuiCol_WindowBg, ImVec4(0.031, 0.055, 0.078, 1.0)); // #080E14
ImGui::PushStyleVar(ImGuiStyleVar_WindowPadding, ImVec2(24.0, 20.0));
ImGui::PushStyleVar(ImGuiStyleVar_WindowRounding, 0.0);
ImGui::PushStyleVar(ImGuiStyleVar_WindowBorderSize, 0.0);

ImGui::Begin("##sidebar", nullptr, 
    ImGuiWindowFlags_NoTitleBar | ImGuiWindowFlags_NoResize | 
    ImGuiWindowFlags_NoMove | ImGuiWindowFlags_NoScrollbar |
    ImGuiWindowFlags_NoBringToDisplayFront);

// Draw cyan left border strip
ImDrawList* dl = ImGui::GetWindowDrawList();
ImVec2 p = ImGui::GetWindowPos();
ImVec2 s = ImGui::GetWindowSize();
dl->AddRectFilled(p, ImVec2(p.x + 3.0f, p.y + s.y), IM_COL32(0, 188, 212, 230));

// ... content here (logo, buttons) ...

ImGui::End();
ImGui::PopStyleVar(3);
ImGui::PopStyleColor(1);
```

Commit: `feat(ui): replace solid panel with glass sidebar`

---

## 🔤 Fix 4 — Font Upgrade

The current font is the default ImGui font (ProggyClean or similar pixel font). Replace with **Barlow Condensed** — a free military-appropriate typeface.

**Download:** https://fonts.google.com/specimen/Barlow+Condensed  
**Files needed:** `BarlowCondensed-Regular.ttf` and `BarlowCondensed-SemiBold.ttf`

Place fonts at: `assets/fonts/BarlowCondensed-Regular.ttf` and `assets/fonts/BarlowCondensed-SemiBold.ttf`

Load in the ImGui font atlas (do this at UI init time, before first frame):
```cpp
ImGuiIO& io = ImGui::GetIO();

// Regular — for version string, labels
ImFont* font_regular = io.Fonts->AddFontFromFileTTF(
    "assets/fonts/BarlowCondensed-Regular.ttf", 16.0f);

// SemiBold — for buttons and logo text  
ImFont* font_semibold = io.Fonts->AddFontFromFileTTF(
    "assets/fonts/BarlowCondensed-SemiBold.ttf", 18.0f);

// Large — for logo/title
ImFont* font_title = io.Fonts->AddFontFromFileTTF(
    "assets/fonts/BarlowCondensed-SemiBold.ttf", 32.0f);

io.Fonts->Build();
```

Use `ImGui::PushFont(font_semibold)` / `ImGui::PopFont()` around button rendering.

If the font files cannot be located automatically at runtime, embed them as byte arrays (use `xxd -i BarlowCondensed-SemiBold.ttf > font_semibold.h` to generate). Add the TTF files to `.gitignore` if they cannot be redistributed; instead add a `scripts/download-fonts.sh` that fetches them from Google Fonts at build time.

Commit: `feat(ui): load Barlow Condensed font for menu UI`

---

## 🔘 Fix 5 — Button Style Overhaul

**Current:** Grey box, white outlined text, no hover animation  
**Target:** Transparent background, white uppercase text, cyan left-accent on hover, slide animation

**Button colour scheme:**

| State | Background | Text | Left accent bar |
|---|---|---|---|
| Default | `rgba(0,0,0,0)` transparent | `rgba(200,210,220,0.85)` | None |
| Hovered | `rgba(0,188,212,0.12)` | `rgba(255,255,255,1.0)` | 3px `rgba(0,188,212,1.0)` |
| Active (clicked) | `rgba(0,188,212,0.25)` | `rgba(255,255,255,1.0)` | 3px `rgba(0,188,212,1.0)` |

**Button sizing:**
- Width: fill sidebar width minus padding (sidebar_width - 48px)
- Height: 42px per button
- Gap between buttons: 6px
- Text: uppercase, letter-spaced (add spaces between letters if ImGui doesn't support letter-spacing natively e.g., `"C A M P A I G N"`)

**Implementation:**
```cpp
ImGui::PushStyleColor(ImGuiCol_Button,        ImVec4(0, 0, 0, 0));
ImGui::PushStyleColor(ImGuiCol_ButtonHovered, ImVec4(0.0f, 0.737f, 0.831f, 0.12f));
ImGui::PushStyleColor(ImGuiCol_ButtonActive,  ImVec4(0.0f, 0.737f, 0.831f, 0.25f));
ImGui::PushStyleColor(ImGuiCol_Text,          ImVec4(0.784f, 0.824f, 0.863f, 0.85f));
ImGui::PushStyleVar(ImGuiStyleVar_FrameRounding, 0.0f);
ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,  ImVec2(12.0f, 11.0f));
ImGui::PushStyleVar(ImGuiStyleVar_ButtonTextAlign, ImVec2(0.0f, 0.5f)); // left-align text

ImFont* btn_font = font_semibold; // from Fix 4
ImGui::PushFont(btn_font);

float btn_width = sidebar_width - 48.0f;

auto draw_menu_button = [&](const char* label, bool is_hovered_externally = false) -> bool {
    ImVec2 btn_pos = ImGui::GetCursorScreenPos();
    bool clicked = ImGui::Button(label, ImVec2(btn_width, 42.0f));
    
    // Draw cyan left accent bar on hover
    if (ImGui::IsItemHovered()) {
        ImDrawList* dl = ImGui::GetWindowDrawList();
        dl->AddRectFilled(
            btn_pos, 
            ImVec2(btn_pos.x + 3.0f, btn_pos.y + 42.0f), 
            IM_COL32(0, 188, 212, 255)
        );
    }
    ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 6.0f); // gap
    return clicked;
};

if (draw_menu_button("  CAMPAIGN"))         { /* launch campaign */ }
if (draw_menu_button("  SINGLE MISSIONS"))  { /* launch single missions */ }
if (draw_menu_button("  MULTIPLAYER"))      { /* launch multiplayer */ }
if (draw_menu_button("  LOCAL MAPS"))       { /* launch local maps */ }
if (draw_menu_button("  MISSION EDITOR"))   { /* launch editor */ }
if (draw_menu_button("  MODS"))             { /* launch mods */ }
if (draw_menu_button("  OPTIONS"))          { /* launch options */ }
if (draw_menu_button("  QUIT"))             { /* quit */ }

ImGui::PopFont();
ImGui::PopStyleVar(3);
ImGui::PopStyleColor(4);
```

Commit: `feat(ui): minimal cyan-accent button style for main menu`

---

## 🏷️ Fix 6 — Logo Placement

**Current:** Logo floats top-left, partially clipped, overlapping awkwardly  
**Target:** Logo sits at the top of the sidebar panel, centred, with proper padding

Inside the sidebar (Fix 3), at the top before buttons:

```cpp
// Logo texture — load from assets/logo.png
// Assuming logo texture ID is stored as `logo_texture`

float logo_size = 80.0f; // render at 80x80
float logo_x = (sidebar_width - logo_size) / 2.0f; // centre horizontally

ImGui::SetCursorPos(ImVec2(logo_x, 24.0f)); // 24px from top
ImGui::Image(logo_texture, ImVec2(logo_size, logo_size));

// PROTEUS title text below logo
ImGui::PushFont(font_title); // 32pt from Fix 4
float title_width = ImGui::CalcTextSize("PROTEUS").x;
ImGui::SetCursorPosX((sidebar_width - title_width) / 2.0f);
ImGui::TextColored(ImVec4(0.784f, 0.863f, 0.925f, 1.0f), "PROTEUS");
ImGui::PopFont();

// Thin separator line
ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 8.0f);
ImDrawList* dl = ImGui::GetWindowDrawList();
ImVec2 sep_start = ImVec2(ImGui::GetWindowPos().x + 24.0f, ImGui::GetCursorScreenPos().y);
ImVec2 sep_end   = ImVec2(ImGui::GetWindowPos().x + sidebar_width - 24.0f, sep_start.y);
dl->AddLine(sep_start, sep_end, IM_COL32(0, 188, 212, 80), 1.0f);
ImGui::SetCursorPosY(ImGui::GetCursorPosY() + 16.0f); // space before first button
```

Commit: `feat(ui): reposition logo to sidebar header with separator`

---

## 📄 Fix 7 — Version String

**Current:** Version string at bottom-left in button font, same visual weight as buttons  
**Target:** Small, muted, positioned at the very bottom of the sidebar

Inside the sidebar, after the buttons — push cursor to the bottom:
```cpp
// Push version string to bottom of sidebar
float footer_height = 40.0f;
ImGui::SetCursorPosY(window_height - footer_height);

ImGui::PushFont(font_regular); // small regular weight
ImGui::PushStyleColor(ImGuiCol_Text, ImVec4(0.4f, 0.5f, 0.55f, 0.8f)); // muted
ImGui::Text("PROTEUS  v%s", version_string); // e.g. "PROTEUS  v3.05-dev"
ImGui::PopStyleColor();
ImGui::PopFont();
```

Remove the separate bottom-bar "PROTEUS – OpenPoseidon engine" text that currently sits outside the sidebar — consolidate everything into the sidebar footer.

Commit: `fix(ui): move version string to sidebar footer, reduce visual weight`

---

## 🖼️ Fix 8 — Logo Asset

Ensure the Proteus logo PNG (the trident/helmet mark, transparent background) is placed at:
```
assets/ui/logo.png
```

If the current logo in the menu is loading from a different path, update the reference. If the logo needs to be added to the asset registry (e.g., listed in a manifest or CMake resource file), do so.

The logo should render at **80×80** in the sidebar header (Fix 6). Do not scale up from a small source — if the source asset is smaller than 256×256, flag it for replacement with a higher-resolution version.

Commit: `asset(ui): register Proteus logo for menu sidebar`

---

## ✅ Completion Checklist

- [ ] Window title reads `Proteus` (or `Proteus [WGPU]`)
- [ ] Background image covers 100% of window
- [ ] Dark vignette gradient applied over background
- [ ] Sidebar is left-aligned, semi-transparent, with cyan left border
- [ ] Buttons are transparent with cyan hover accent and left-align text
- [ ] Barlow Condensed font loaded and applied
- [ ] Logo centred in sidebar header at 80px with title text below
- [ ] Thin cyan separator between logo/title and buttons
- [ ] Version string at sidebar footer, muted, small font
- [ ] No solid grey panels remaining anywhere in the menu

After completing all fixes, **take a screenshot of the result** (or describe the render output precisely) and report back for review.

---

## ⚠️ Do Not Change

- Button labels / names (CAMPAIGN, SINGLE MISSIONS, etc.) — keep as-is
- Button functionality / click handlers — visual only, no logic changes
- Any code outside the main menu rendering path
- The background image asset itself

---

*Proteus — UI Overhaul — Oct 2026*
