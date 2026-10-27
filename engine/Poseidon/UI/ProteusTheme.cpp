#include <Poseidon/UI/ProteusTheme.hpp>

#include <Poseidon/UI/Controls/UIControls.hpp>
#include <Poseidon/Core/resincl.hpp>
#include <Poseidon/Foundation/Common/PlatformPaths.hpp>
#include <Poseidon/Foundation/Common/Win.h>
#include <Poseidon/Graphics/Core/Engine.hpp>
#include <Poseidon/Graphics/Textures/TexturePreload.hpp>
#include <Poseidon/Graphics/Textures/TextureBank.hpp>
#include <Poseidon/World/World.hpp>

#include <algorithm>
#include <cctype>
#include <filesystem>
#include <string>

namespace Poseidon
{
namespace ProteusTheme
{
namespace
{
// Reference height the pixel values below were tuned at. Positions are converted to screen
// fractions against the live window, so the layout scales with the window.
constexpr float kRefW = 816.0f;
constexpr float kRefH = 639.0f;

// Proteus art ships in the executable's assets\ui folder. FindPicture accepts absolute paths,
// so resolve it there. Empty when the file is missing and the picture simply isn't drawn.
RString ArtPath(const char* file)
{
    const std::string exePath = Poseidon::Foundation::getExecutablePath();
    if (exePath.empty())
        return RString();
    const std::filesystem::path path = std::filesystem::path(exePath).parent_path() / "assets" / "ui" / file;
    if (!std::filesystem::exists(path))
        return RString();
    return RString(path.string().c_str());
}

// Empty when the file is missing; GlobLoadTexture is not called with an empty name.
Ref<Texture> LoadArt(const char* file)
{
    const RString path = ArtPath(file);
    return path.GetLength() > 0 ? GlobLoadTexture(path) : Ref<Texture>();
}

void SetColour(ParamClass* cls, const char* name, Rgba c)
{
    ParamEntry* arr = cls->AddArray(name);
    arr->AddValue(c.r);
    arr->AddValue(c.g);
    arr->AddValue(c.b);
    arr->AddValue(c.a);
}

// Adds a class with the given name and optional text, and returns it for the caller to fill.
ParamClass* NewClass(ParamFile& cfg, const char* name, const char* text)
{
    ParamClass* cls = cfg.AddClass(name);
    if (text)
        cls->Add("text", RString(text));
    return cls;
}

void SetIdc(ParamClass* cls, int idc)
{
    cls->Add("idc", idc);
}

// Pixel values are on the 816x639 design canvas; the screen arguments are kept for call sites
// but the fractions are taken against the design canvas.
float FracX(float px, float) { return px / kRefW; }
float FracY(float px, float) { return px / kRefH; }
} // namespace

Metrics MetricsFor(float screenW, float screenH)
{
    Metrics m;
    m.sidebarW = std::clamp(screenW * SIDEBAR_WIDTH_RATIO, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
    if (screenH < 600.0f)
    {
        m.btnH = 36.0f;
        m.fontScale = 0.9f;
    }
    else if (screenH < 900.0f)
    {
        m.btnH = 42.0f;
        m.fontScale = 1.0f;
    }
    else
    {
        m.btnH = 48.0f;
        m.fontScale = 1.1f;
    }
    return m;
}

void Colour(ParamClass* cls, const char* name, Rgba v)
{
    ParamEntry* arr = cls->AddArray(name);
    arr->AddValue(v.r);
    arr->AddValue(v.g);
    arr->AddValue(v.b);
    arr->AddValue(v.a);
}

ParamClass* NewControl(ParamFile& cfg, const char* name, int idc, int type, const char* text, float size, float x,
                       float y, float w, float h, Rgba ink)
{
    ParamClass* cls = cfg.AddClass(name);
    cls->Add("idc", idc);
    cls->Add("type", type);
    cls->Add("text", RString(text));
    cls->Add("style", ST_LEFT);
    cls->Add("font", RString("barlowsemibold"));
    cls->Add("sizeEx", size);
    cls->Add("x", x);
    cls->Add("y", y);
    cls->Add("w", w);
    cls->Add("h", h);
    Colour(cls, "colorBackground", Rgba{0.0f, 0.0f, 0.0f, 0.0f});
    Colour(cls, "colorText", ink);
    return cls;
}

ParamClass* NewRule(ParamFile& cfg, const char* name, int idc, float x, float y, float w)
{
    ParamClass* rule = cfg.AddClass(name);
    rule->Add("idc", idc);
    rule->Add("text", RString(""));
    rule->Add("type", CT_STATIC);
    rule->Add("style", ST_LEFT);
    rule->Add("x", x);
    rule->Add("y", y);
    rule->Add("w", w);
    rule->Add("h", 1.0f / 639.0f);
    Colour(rule, "colorBackground", ACCENT_DIM);
    Colour(rule, "colorText", TEXT_PRIMARY);
    return rule;
}

void AddHtmlStyles(ParamClass* html)
{
    struct Style
    {
        const char* tag;
        float size;
    };
    const Style styles[] = {{"H1", 0.040f}, {"H2", 0.038f}, {"H3", 0.036f}, {"H4", 0.034f},
                            {"H5", 0.032f}, {"H6", 0.032f}, {"P", 0.030f}};
    for (const Style& s : styles)
    {
        ParamClass* cls = html->AddClass(s.tag);
        cls->Add("font", RString("barlowregular"));
        cls->Add("fontBold", RString("barlowsemibold"));
        cls->Add("sizeEx", s.size);
    }
}

void DrawBackdropArt()
{
    // Loading starts before the renderer and its texture bank exist; nothing is drawn until they do.
    if (!GLOB_ENGINE || !GLOB_ENGINE->TextBank())
        return;
    static Ref<Texture> background = LoadArt("proteus_background.paa");
    static Ref<Texture> vignette = LoadArt("vignette.paa");
    const float w = GLOB_ENGINE->Width2D();
    const float h = GLOB_ENGINE->Height2D();
    if (background)
    {
        MipInfo mip = GLOB_ENGINE->TextBank()->UseMipmap(background, 0, 0);
        GLOB_ENGINE->Draw2D(mip, PackedWhite, Rect2DPixel(0, 0, w, h));
    }
    if (vignette)
    {
        MipInfo mip = GLOB_ENGINE->TextBank()->UseMipmap(vignette, 0, 0);
        GLOB_ENGINE->Draw2D(mip, PackedWhite, Rect2DPixel(0, 0, w, h));
    }
    // The same light wash as the menu backdrop, so text reads over the art.
    Texture* white = GLOB_SCENE->Preloaded(TextureWhite);
    MipInfo washMip = GLOB_ENGINE->TextBank()->UseMipmap(white, 0, 0);
    GLOB_ENGINE->Draw2D(washMip, PackedColor(Color(0.02f, 0.03f, 0.05f, 0.30f)), Rect2DPixel(0, 0, w, h));
}

RString Upper(RString text)
{
    std::string s = (const char*)text;
    for (char& c : s)
        c = static_cast<char>(toupper(static_cast<unsigned char>(c)));
    return RString(s.c_str());
}

void MenuButton::OnDraw(float alpha)
{
    const int screenW = GLOB_ENGINE->Width2D();
    const int screenH = GLOB_ENGINE->Height2D();
    const float xx = static_cast<int>(_x * screenW) + 0.5f;
    const float yy = static_cast<int>(_y * screenH) + 0.5f;
    const float ww = static_cast<int>(_w * screenW);
    const float hh = static_cast<int>(_h * screenH);
    const bool hot = IsFocused();
    const bool pushed = IsEnabled() && _state;

    if (hot)
    {
        Texture* white = GLOB_SCENE->Preloaded(TextureWhite);
        const Rgba wash = pushed ? BTN_ACTIVE_WASH : BTN_HOVER_WASH;
        MipInfo washMip = GLOB_ENGINE->TextBank()->UseMipmap(white, 0, 0);
        GLOB_ENGINE->Draw2D(washMip, PackedColor(Color(wash.r, wash.g, wash.b, wash.a * alpha)),
                            Rect2DPixel(xx, yy, ww, hh));
        MipInfo barMip = GLOB_ENGINE->TextBank()->UseMipmap(white, 0, 0);
        GLOB_ENGINE->Draw2D(barMip, PackedColor(Color(ACCENT.r, ACCENT.g, ACCENT.b, alpha)),
                            Rect2DPixel(xx, yy, ACCENT_BAR_WIDTH, hh));
    }

    const float padding = 12.0f / screenW;
    const float slide = hot ? BTN_FOCUS_SHIFT / screenW : 0.0f;
    const float left = _x + padding + slide;
    const float top = _y + 0.5f * (_h - _size);
    const Rgba text = hot ? Rgba{1.0f, 1.0f, 1.0f, 1.0f} : TEXT_PRIMARY;
    GLOB_ENGINE->DrawText(Point2DFloat(left, top), _size, Rect2DFloat(_x, _y, _w, _h), _font,
                          PackedColor(Color(text.r, text.g, text.b, text.a * alpha)), _text);
}

void AddArtBackdrop(ControlsContainer* parent, ParamFile& cfg, int idcBase)
{
    auto fullScreen = [&](ParamClass* cls, Rgba bg)
    {
        cls->Add("x", 0.0f);
        cls->Add("y", 0.0f);
        cls->Add("w", 1.0f);
        cls->Add("h", 1.0f);
        SetColour(cls, "colorBackground", bg);
        SetColour(cls, "colorText", Rgba{1.0f, 1.0f, 1.0f, 1.0f});
    };

    // Key art across the whole window.
    ParamClass* art = NewClass(cfg, "ProteusArt", nullptr);
    SetIdc(art, idcBase + 0);
    art->Add("type", CT_STATIC);
    art->Add("style", ST_PICTURE);
    art->Add("text", ArtPath("proteus_background.paa"));
    fullScreen(art, Rgba{0.0f, 0.0f, 0.0f, 0.0f});
    parent->LoadControl(*art);

    // Dark edge vignette over the art. Transparent in the centre.
    ParamClass* vignette = NewClass(cfg, "ProteusVignette", nullptr);
    SetIdc(vignette, idcBase + 1);
    vignette->Add("type", CT_STATIC);
    vignette->Add("style", ST_PICTURE);
    vignette->Add("text", ArtPath("vignette.paa"));
    fullScreen(vignette, Rgba{0.0f, 0.0f, 0.0f, 0.0f});
    parent->LoadControl(*vignette);

    // Light full-window wash for readability behind the content.
    ParamClass* wash = NewClass(cfg, "ProteusWash", "");
    SetIdc(wash, idcBase + 2);
    wash->Add("type", CT_STATIC);
    wash->Add("style", ST_LEFT);
    fullScreen(wash, Rgba{0.02f, 0.03f, 0.05f, 0.30f});
    parent->LoadControl(*wash);
}

void AddSidebarPanel(ControlsContainer* parent, ParamFile& cfg, int idcBase)
{
    const float screenW = GLOB_ENGINE->Width2D();
    const float screenH = GLOB_ENGINE->Height2D();
    const float sidebarFrac = MetricsFor(screenW, screenH).sidebarW / screenW;

    // Glass sidebar on the left edge, with a 3px cyan border on its left edge.
    ParamClass* panel = NewClass(cfg, "ProteusPanel", "");
    SetIdc(panel, idcBase + 0);
    panel->Add("type", CT_STATIC);
    panel->Add("style", ST_LEFT);
    panel->Add("x", 0.0f);
    panel->Add("y", 0.0f);
    panel->Add("w", sidebarFrac);
    panel->Add("h", 1.0f);
    SetColour(panel, "colorBackground", PANEL_BG);
    SetColour(panel, "colorText", Rgba{1.0f, 1.0f, 1.0f, 1.0f});
    parent->LoadControl(*panel);

    ParamClass* accent = NewClass(cfg, "ProteusAccent", "");
    SetIdc(accent, idcBase + 1);
    accent->Add("type", CT_STATIC);
    accent->Add("style", ST_LEFT);
    accent->Add("x", 0.0f);
    accent->Add("y", 0.0f);
    accent->Add("w", ACCENT_BAR_WIDTH / screenW);
    accent->Add("h", 1.0f);
    SetColour(accent, "colorBackground", Rgba{ACCENT.r, ACCENT.g, ACCENT.b, 0.9f});
    SetColour(accent, "colorText", Rgba{1.0f, 1.0f, 1.0f, 1.0f});
    parent->LoadControl(*accent);
}

void AddBackdrop(ControlsContainer* parent, ParamFile& cfg, int idcBase)
{
    AddArtBackdrop(parent, cfg, idcBase);
    AddSidebarPanel(parent, cfg, idcBase + 3);
}

void AddCentredPanel(ControlsContainer* parent, ParamFile& cfg, int idcBase, float x, float y, float w, float h)
{
    const float screenH = GLOB_ENGINE->Height2D();

    ParamClass* panel = NewClass(cfg, "ProteusCentredPanel", "");
    SetIdc(panel, idcBase + 0);
    panel->Add("type", CT_STATIC);
    panel->Add("style", ST_LEFT);
    panel->Add("x", x);
    panel->Add("y", y);
    panel->Add("w", w);
    panel->Add("h", h);
    SetColour(panel, "colorBackground", PANEL_BG);
    SetColour(panel, "colorText", Rgba{1.0f, 1.0f, 1.0f, 1.0f});
    parent->LoadControl(*panel);

    // 3px cyan top border across the panel.
    ParamClass* top = NewClass(cfg, "ProteusCentredTop", "");
    SetIdc(top, idcBase + 1);
    top->Add("type", CT_STATIC);
    top->Add("style", ST_LEFT);
    top->Add("x", x);
    top->Add("y", y);
    top->Add("w", w);
    top->Add("h", ACCENT_BAR_WIDTH / screenH);
    SetColour(top, "colorBackground", ACCENT);
    SetColour(top, "colorText", Rgba{1.0f, 1.0f, 1.0f, 1.0f});
    parent->LoadControl(*top);
}

void AddSidebarHeader(ControlsContainer* parent, ParamFile& cfg, int idcBase)
{
    const float screenW = GLOB_ENGINE->Width2D();
    const float screenH = GLOB_ENGINE->Height2D();
    const float headerW = MetricsFor(screenW, screenH).sidebarW / screenW;

    // 80x80 logo, centred, 24px from the top.
    const float logoW = FracX(80.0f, screenW);
    const float logoH = FracY(80.0f, screenH);
    ParamClass* logo = NewClass(cfg, "ProteusLogo", nullptr);
    SetIdc(logo, idcBase + 5);
    logo->Add("type", CT_STATIC);
    logo->Add("style", ST_PICTURE);
    logo->Add("text", ArtPath("logo.png"));
    logo->Add("x", (headerW - logoW) * 0.5f);
    logo->Add("y", FracY(24.0f, screenH));
    logo->Add("w", logoW);
    logo->Add("h", logoH);
    SetColour(logo, "colorBackground", Rgba{0.0f, 0.0f, 0.0f, 0.0f});
    SetColour(logo, "colorText", Rgba{1.0f, 1.0f, 1.0f, 1.0f});
    parent->LoadControl(*logo);

    // Title beneath the logo.
    ParamClass* title = NewClass(cfg, "ProteusTitle", "PROTEUS");
    SetIdc(title, idcBase + 6);
    title->Add("type", CT_STATIC);
    title->Add("style", ST_CENTER);
    title->Add("font", RString("barlowsemibold"));
    title->Add("sizeEx", 0.05f);
    title->Add("x", 0.0f);
    title->Add("y", FracY(112.0f, screenH));
    title->Add("w", headerW);
    title->Add("h", FracY(40.0f, screenH));
    SetColour(title, "colorBackground", Rgba{0.0f, 0.0f, 0.0f, 0.0f});
    SetColour(title, "colorText", TITLE_TEXT);
    parent->LoadControl(*title);

    // 1px cyan separator, inset 24px each side.
    ParamClass* separator = NewClass(cfg, "ProteusSeparator", "");
    SetIdc(separator, idcBase + 7);
    separator->Add("type", CT_STATIC);
    separator->Add("style", ST_LEFT);
    separator->Add("x", FracX(PANEL_PADDING_H, screenW));
    separator->Add("y", FracY(160.0f, screenH));
    separator->Add("w", headerW - 2.0f * FracX(PANEL_PADDING_H, screenW));
    separator->Add("h", FracY(1.0f, screenH));
    SetColour(separator, "colorBackground", ACCENT_DIM);
    SetColour(separator, "colorText", Rgba{1.0f, 1.0f, 1.0f, 1.0f});
    parent->LoadControl(*separator);
}

void AddSidebarFooter(ControlsContainer* parent, ParamFile& cfg, int idcBase, const RString& version)
{
    const float screenW = GLOB_ENGINE->Width2D();
    const float screenH = GLOB_ENGINE->Height2D();
    const float headerW = MetricsFor(screenW, screenH).sidebarW / screenW;

    // Keep the first token of the build string, e.g. "3.05-da00252-dev".
    const std::string fullVersion = (const char*)version;
    const std::string shortVersion = fullVersion.substr(0, fullVersion.find(' '));

    ParamClass* footer = NewClass(cfg, "ProteusFooter", nullptr);
    SetIdc(footer, idcBase + 8);
    footer->Add("type", CT_STATIC);
    footer->Add("style", ST_LEFT);
    footer->Add("font", RString("barlowregular"));
    footer->Add("sizeEx", 0.026f);
    footer->Add("text", RString(("PROTEUS  v" + shortVersion).c_str()));
    footer->Add("x", FracX(PANEL_PADDING_H, screenW));
    footer->Add("y", FracY(kRefH - 40.0f, screenH)); // 40px band at the bottom of the sidebar
    footer->Add("w", headerW - 2.0f * FracX(PANEL_PADDING_H, screenW));
    footer->Add("h", FracY(32.0f, screenH));
    SetColour(footer, "colorBackground", Rgba{0.0f, 0.0f, 0.0f, 0.0f});
    SetColour(footer, "colorText", TEXT_MUTED);
    parent->LoadControl(*footer);
}

float SidebarButtonTop(int index, float)
{
    // First button sits 16px below the separator at 160px; buttons stack with a 6px gap.
    const float firstTop = 176.0f;
    const float step = 42.0f + BTN_GAP;
    return (firstTop + step * index) / kRefH;
}

float SidebarButtonHeight(float screenW, float screenH)
{
    return MetricsFor(screenW, screenH).btnH / kRefH;
}

} // namespace ProteusTheme
} // namespace Poseidon
