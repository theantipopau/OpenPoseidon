#include <Poseidon/World/Model/ModelCache.hpp>
#include <Poseidon/Asset/Formats/Enfusion/EnfusionMount.hpp>
#include <Poseidon/Core/Config/EngineConfig.hpp>
#include <Poseidon/Dev/Diag/PhysicsProbe.hpp>
#include <Poseidon/Foundation/Logging/Logging.hpp>
#include <Poseidon/Foundation/Platform/AppConfig.hpp>
#include <Poseidon/Core/Config/Config.hpp>
#include <Poseidon/Foundation/Containers/Array.hpp>
#include <Poseidon/Foundation/Strings/RString.hpp>
#include <Poseidon/Foundation/platform.hpp>

#ifdef _WIN32
#include <Poseidon/Foundation/Common/Win.h>
#include <io.h>
#include <process.h>
#else
#include <unistd.h>
#endif
#include <Poseidon/Core/resrc1.h>

#include <Poseidon/Core/Global.hpp>

#include <Poseidon/Core/Application.hpp>

using namespace Poseidon;

extern void AppPause(bool f);
namespace Poseidon
{
extern void DestroyEngine();
}

#include <Poseidon/Core/Game/GameLoop.hpp>

namespace Poseidon
{
extern RString GetPidFileName();
extern bool GetMyPidFile();
} // namespace Poseidon

#include <Poseidon/World/World.hpp>
#include <Poseidon/Graphics/Core/Engine.hpp>
#include <Poseidon/AI/AI.hpp>
#include <Poseidon/AI/ArcadeTemplate.hpp>

#include <Poseidon/Dev/Debug/DebugTrap.hpp>

using namespace Poseidon::Dev;

#include <Poseidon/Audio/IAudioSystem.hpp>
#include <Poseidon/Audio/SoundScene.hpp>
#include <Poseidon/Graphics/Rendering/Draw/FontSystem.hpp>
#include <Poseidon/World/Scene/ScenePreloader.hpp>

#include <Poseidon/IO/ParamFile/ParamFile.hpp>

#include <Poseidon/UI/Locale/StringtableExt.hpp>
#include <Poseidon/Network/Network.hpp>
#include <Poseidon/Network/NetworkConfig.hpp>

#include <chrono>

namespace Poseidon
{
struct ::ArcadeTemplate& GetGClipboard();
}

static bool MouseEx = true;

// Win32 HINSTANCE — used by Dev/Debug/DebugWin.cpp for CreateWindowEx
#ifdef _WIN32
HINSTANCE GHInstance = nullptr;
#endif

static bool fMouseAcquired = true;
bool IsMouseAcquired()
{
    return fMouseAcquired;
}
void SetMouseAcquired(bool acquired)
{
    fMouseAcquired = acquired;
}

#if _ENABLE_CHEATS
#include <Poseidon/Input/KeyLights.hpp>

KeyLights KeyState;
#endif

bool Benchmark = false;
bool AutoTest = false;

const char* ErrorString;

#ifdef _WIN32
RECT clientRect;
#else
struct
{
    long left, top, right, bottom;
} clientRect;
#endif

char LoadFile[512];

extern float Height;

ArcadeTemplate& GetGClipboard();

void ManCleanUp();

namespace
{
// Elapsed-time probe for the 30-70 s shutdown windows seen in quit logs: each phase of
// UnloadGameData logs a SHUTDOWN-TIME line at INFO so the next quit run shows exactly
// where the time goes. Behaviour-neutral (RAII log on scope exit only).
class ShutdownPhaseTimer
{
  public:
    explicit ShutdownPhaseTimer(const char* phase) : _phase(phase), _start(std::chrono::steady_clock::now()) {}
    ~ShutdownPhaseTimer()
    {
        const long long ms =
            std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - _start).count();
        LOG_INFO(Core, "SHUTDOWN-TIME phase '{}' took {} ms", _phase, ms);
    }

  private:
    const char* _phase;
    std::chrono::steady_clock::time_point _start;
};
} // namespace

// Tears down the whole game-data layer (world, scene, sound, banks, configs,
// caches) in the order the engine has always used. When keepEngine is true the
// graphics engine — the SDL window + RHI device — is left alive; that single
// skipped DestroyEngine() is the only difference between a mod re-mount (which
// reloads onto the same window) and a quit-to-desktop.
void UnloadGameData(bool keepEngine)
{
    ShutdownPhaseTimer total("unload-game-data-total");
    GDebugger.PauseCheckingAlive();
    if (GEngine)
    {
        GEngine->ReinitCounters();
    }

    {
        ShutdownPhaseTimer phase("render-stop");
        GApp->m_canRender = false;
        markersMap.Clear();
        CurrentTemplate.Clear();
        Poseidon::GetGClipboard().Clear();
        if (GEngine)
        {
            GEngine->StopAll();
        }
    }
    // Probe pools otherwise release GPU meshes from static destructors after
    // the renderer/uploader has already gone away.
    Dev::ShutdownPhysicsProbes();
    {
        ShutdownPhaseTimer phase("delete-world");
        if (GWorld)
        {
            delete GWorld, GWorld = nullptr;
        }
    }
    // GScene aliases GWorld->_scene (a value member of World, assigned via GScene = GetScene()).
    // Deleting GWorld already destroyed that Scene; GScene is not separately heap-allocated, so
    // it must only be cleared here — a delete is a double-free + use-after-free of the World
    // (Scene::~Scene re-runs the landscape teardown and SaveConfig() reads the freed World).
    {
        ShutdownPhaseTimer phase("mounts-caches");
        ModelCache::SetExternalLoader(nullptr);
        Poseidon::Asset::Formats::Enfusion::EnfusionMount::Instance().Close();
        GScene = nullptr;
        // The next Scene needs its own preload slots after an in-process remount.
        ScenePreloader::Instance().Shutdown();
        if (GEngine)
        {
            GEngine->ClearFontCache();
        }
        FontSystem::Instance().Shutdown();
        GPreloadedTextures.Clear();
    }
    // Release the sound scene's waves (env sounds, global sounds, music track) while the
    // sound system that owns their OpenAL buffers/sources is still alive. GSoundScene is a
    // global that outlives a re-mount (only CleanupSoundSystem deletes it, and that runs in
    // DestroyEngine, which keepEngine skips). Without this, the waves linger past the
    // delete GSoundsys below; the next World ctor's GSoundScene->Reset() then destroys them
    // and ~WaveOAL calls SoundSystemOAL::UnregisterWave on the freed system (AV in std::find).
    {
        ShutdownPhaseTimer phase("sound");
        if (GSoundScene)
        {
            GSoundScene->Reset();
        }
        if (GSoundsys)
        {
            delete GSoundsys, GSoundsys = nullptr;
        }
    }
    {
        ShutdownPhaseTimer phase("configs-banks");
        ClearStringtable();
        ManCleanUp();
        Glob.Clear();
    }
    {
        ShutdownPhaseTimer phase("destroy-engine");
        if (!keepEngine)
        {
            Poseidon::DestroyEngine();
        }
    }
    {
        ShutdownPhaseTimer phase("pars-res-network");
        Pars.Clear();
        Pars.DeleteVariables();
        ExtParsCampaign.Clear();
        ExtParsCampaign.DeleteVariables();
        ExtParsMission.Clear();
        ExtParsMission.DeleteVariables();
        Res.Clear();
        Res.DeleteVariables();
        GetNetworkManager().Done();
    }
}

void DDTerm()
{
    UnloadGameData(/*keepEngine*/ false);
    if (Poseidon::GetMyPidFile())
    {
        DeleteFile((const char*)Poseidon::GetPidFileName());
    }
}
