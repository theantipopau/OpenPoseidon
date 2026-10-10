#pragma once

#include <Poseidon/UI/Controls/UIControlsWidgets.hpp>
#include <Poseidon/IO/ParamFile/ParamFile.hpp>

namespace Poseidon
{
// Proteus design system: the colour and size tokens, the shared button, and the backdrop and
// sidebar pieces every themed screen uses. A screen opts in with ControlsContainer::
// EnableProteusTheme(), then calls AddBackdrop() before Load() so the backdrop sits behind the
// retail controls. Buttons created on a themed screen are ProteusTheme::MenuButton.
namespace ProteusTheme
{
struct Rgba
{
    float r, g, b, a;
};

// Colours (spec tokens, as 0..1 floats).
constexpr Rgba PANEL_BG{8.0f / 255.0f, 14.0f / 255.0f, 20.0f / 255.0f, 0.82f};
constexpr Rgba ACCENT{0.0f, 188.0f / 255.0f, 212.0f / 255.0f, 1.0f};
constexpr Rgba ACCENT_DIM{0.0f, 188.0f / 255.0f, 212.0f / 255.0f, 0.31f};
constexpr Rgba TEXT_PRIMARY{200.0f / 255.0f, 215.0f / 255.0f, 225.0f / 255.0f, 0.85f};
constexpr Rgba TEXT_MUTED{100.0f / 255.0f, 130.0f / 255.0f, 140.0f / 255.0f, 0.8f};
constexpr Rgba BTN_HOVER_WASH{0.0f, 188.0f / 255.0f, 212.0f / 255.0f, 0.12f};
constexpr Rgba BTN_ACTIVE_WASH{0.0f, 188.0f / 255.0f, 212.0f / 255.0f, 0.25f};
constexpr Rgba TITLE_TEXT{0.784f, 0.863f, 0.925f, 1.0f};

// Dimensions in pixels at the standard tier (window height 600 to 899).
constexpr float SIDEBAR_WIDTH_RATIO = 0.32f;
constexpr float SIDEBAR_MIN_WIDTH = 260.0f;
constexpr float SIDEBAR_MAX_WIDTH = 380.0f;
constexpr float ACCENT_BAR_WIDTH = 3.0f;
constexpr float BTN_GAP = 6.0f;
constexpr float PANEL_PADDING_H = 24.0f;
constexpr float PANEL_PADDING_V = 20.0f;
constexpr float BTN_FOCUS_SHIFT = 4.0f;

// Resolution tiers (spec Step 3). Button height and font scale follow the window height.
struct Metrics
{
    float sidebarW; // px, clamped to SIDEBAR_MIN_WIDTH..SIDEBAR_MAX_WIDTH
    float btnH;     // px
    float fontScale;
};
Metrics MetricsFor(float screenW, float screenH);

// Button for themed screens: transparent until focused. Focus (mouse or keyboard) shows a cyan
// wash, a 3px left accent and a 4px label slide. The label and action are CButton's.
class MenuButton : public CButton
{
public:
    using CButton::CButton;
    void OnDraw(float alpha) override;
};

// Full-bleed background art, dark edge vignette, and the sidebar panel with its cyan left
// border. Adds five controls to the screen's foreground, so call before Load(). The screen owns
// idcBase..idcBase+4 (use the 9600+ range, unique per screen).
void AddBackdrop(ControlsContainer* parent, ParamFile& cfg, int idcBase);

// Full-bleed art, vignette and wash only, for screens without a sidebar. Owns idcBase..idcBase+2.
void AddArtBackdrop(ControlsContainer* parent, ParamFile& cfg, int idcBase);

// Centred panel with a 3px cyan top border, for settings screens. Coordinates are screen
// fractions. Owns idcBase and idcBase+1.
void AddCentredPanel(ControlsContainer* parent, ParamFile& cfg, int idcBase, float x, float y, float w, float h);

// Logo, PROTEUS title and cyan separator across the top of the sidebar. Owns idcBase+5..+7.
void AddSidebarHeader(ControlsContainer* parent, ParamFile& cfg, int idcBase);

// Muted version line at the bottom of the sidebar, "PROTEUS  v<build>". Owns idcBase+8.
void AddSidebarFooter(ControlsContainer* parent, ParamFile& cfg, int idcBase, const RString& version);

// Top edge and height of sidebar button `index` in screen fractions (16px below the separator).
float SidebarButtonTop(int index, float screenH);
float SidebarButtonHeight(float screenW, float screenH);

} // namespace ProteusTheme
} // namespace Poseidon
