// Tests for cross-platform path resolution in QStream / file bank layer.
// Verifies that CmpStartStr, FindFileInfo, ConvertDirSlash, SetPrefix,
// AutoBank, QIFStream::open, and FileExist all handle mixed-slash and
// mixed-case paths correctly -- matching Windows case-insensitive semantics
// on Linux.

#include <catch2/catch_test_macros.hpp>
#include <Poseidon/Core/ModSystem.hpp>
#include <Poseidon/IO/Filesystem/Utf8Paths.hpp>
#include <Poseidon/IO/Streams/QBStream.hpp>
#include <Poseidon/IO/Streams/QStream.hpp>
#include "../Support/test_fixtures.hpp"
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>
#include <system_error>
#include <Poseidon/Foundation/Strings/RString.hpp>

using namespace TestFixtures;

// CmpStartStr – case-insensitive, slash-agnostic prefix comparison

TEST_CASE("CmpStartStr - exact match", "[qstream][path][cmpstart]")
{
    REQUIRE(CmpStartStr("Data3D\\sky_plane.p3d", "Data3D\\") == 0);
}

TEST_CASE("CmpStartStr - case insensitive", "[qstream][path][cmpstart]")
{
    REQUIRE(CmpStartStr("data3d\\sky_plane.p3d", "Data3D\\") == 0);
    REQUIRE(CmpStartStr("DATA3D\\OBLOHA.P3D", "data3d\\") == 0);
    REQUIRE(CmpStartStr("DtA\\file.bin", "dTa\\") == 0);
}

TEST_CASE("CmpStartStr - slash agnostic", "[qstream][path][cmpstart]")
{
    // Forward slash in string vs backslash in prefix
    REQUIRE(CmpStartStr("Data3D/sky_plane.p3d", "Data3D\\") == 0);
    // Backslash in string vs forward slash in prefix
    REQUIRE(CmpStartStr("Data3D\\sky_plane.p3d", "Data3D/") == 0);
}

TEST_CASE("CmpStartStr - mixed case and slash", "[qstream][path][cmpstart]")
{
    REQUIRE(CmpStartStr("data3d/sky_plane.p3d", "Data3D\\") == 0);
    REQUIRE(CmpStartStr("DATA3D\\FILE.BIN", "data3d/") == 0);
    REQUIRE(CmpStartStr("dta/config.bin", "DTA\\") == 0);
}

TEST_CASE("CmpStartStr - mismatch returns nonzero", "[qstream][path][cmpstart]")
{
    REQUIRE(CmpStartStr("Abel\\file.txt", "Cain\\") != 0);
    REQUIRE(CmpStartStr("data3d\\file.txt", "data4d\\") != 0);
}

TEST_CASE("CmpStartStr - empty prefix matches anything", "[qstream][path][cmpstart]")
{
    REQUIRE(CmpStartStr("anything", "") == 0);
    REQUIRE(CmpStartStr("", "") == 0);
}

TEST_CASE("CmpStartStr - nested path separators", "[qstream][path][cmpstart]")
{
    REQUIRE(CmpStartStr("a/b/c/d.txt", "a\\b\\") == 0);
    REQUIRE(CmpStartStr("a\\b\\c\\d.txt", "a/b/") == 0);
    REQUIRE(CmpStartStr("A/B/C/D.txt", "a\\b\\c\\") == 0);
}

// QFBank::SetPrefix – normalizes separators for the platform

TEST_CASE("QFBank SetPrefix - stores prefix", "[qstream][path][bank]")
{
    QFBank bank;
    bank.SetPrefix("TestBank\\");

#ifdef _WIN32
    REQUIRE(strcmp(bank.GetPrefix().Data(), "TestBank\\") == 0);
#else
    // On Linux, backslash in prefix is normalized to forward slash
    REQUIRE(strcmp(bank.GetPrefix().Data(), "TestBank/") == 0);
#endif
}

TEST_CASE("QFBank SetPrefix - forward slash preserved on Linux", "[qstream][path][bank]")
{
    QFBank bank;
    bank.SetPrefix("DTA/");

#ifdef _WIN32
    REQUIRE(strcmp(bank.GetPrefix().Data(), "DTA\\") == 0);
#else
    REQUIRE(strcmp(bank.GetPrefix().Data(), "DTA/") == 0);
#endif
}

TEST_CASE("QFBank SetPrefix - mixed slashes normalized", "[qstream][path][bank]")
{
    QFBank bank;
    bank.SetPrefix("path/to\\bank\\");

#ifdef _WIN32
    REQUIRE(strcmp(bank.GetPrefix().Data(), "path\\to\\bank\\") == 0);
#else
    REQUIRE(strcmp(bank.GetPrefix().Data(), "path/to/bank/") == 0);
#endif
}

// AutoBank – prefix matching against registered banks

TEST_CASE("AutoBank - matches with backslash prefix", "[qstream][path][autobank]")
{
    int origSize = GFileBanks.Size();

    // Register a test bank with backslash prefix (as PBO loading does)
    int idx = GFileBanks.Add();
    QFBank& bank = GFileBanks.Set(idx);
    bank.SetPrefix("TestData\\");

    // Should match regardless of slash direction in query
#ifdef _WIN32
    REQUIRE(QIFStreamB::AutoBank("TestData\\config.bin") == &bank);
#else
    // SetPrefix normalizes to "TestData/" on Linux
    REQUIRE(QIFStreamB::AutoBank("TestData/config.bin") == &bank);
#endif

    // Cleanup
    GFileBanks.Delete(idx);
    REQUIRE(GFileBanks.Size() == origSize);
}

TEST_CASE("AutoBank - case insensitive match", "[qstream][path][autobank]")
{
    int origSize = GFileBanks.Size();

    int idx = GFileBanks.Add();
    QFBank& bank = GFileBanks.Set(idx);
    bank.SetPrefix("Data3D\\");

#ifdef _WIN32
    const char* expected_prefix_sep = "\\";
#else
    const char* expected_prefix_sep = "/";
#endif

    // Case variants should all match
    std::string query1 = std::string("data3d") + expected_prefix_sep + "sky_plane.p3d";
    std::string query2 = std::string("DATA3D") + expected_prefix_sep + "sky_plane.p3d";
    std::string query3 = std::string("Data3D") + expected_prefix_sep + "OBLOHA.P3D";

    REQUIRE(QIFStreamB::AutoBank(query1.c_str()) == &bank);
    REQUIRE(QIFStreamB::AutoBank(query2.c_str()) == &bank);
    REQUIRE(QIFStreamB::AutoBank(query3.c_str()) == &bank);

    GFileBanks.Delete(idx);
    REQUIRE(GFileBanks.Size() == origSize);
}

TEST_CASE("AutoBank - cross-slash match", "[qstream][path][autobank]")
{
    int origSize = GFileBanks.Size();

    int idx = GFileBanks.Add();
    QFBank& bank = GFileBanks.Set(idx);
    bank.SetPrefix("Anims\\");

    // CmpStartStr treats / and \ as equivalent
    REQUIRE(QIFStreamB::AutoBank("Anims/walk.rtm") == &bank);
    REQUIRE(QIFStreamB::AutoBank("Anims\\walk.rtm") == &bank);
    REQUIRE(QIFStreamB::AutoBank("anims/walk.rtm") == &bank);
    REQUIRE(QIFStreamB::AutoBank("ANIMS\\WALK.RTM") == &bank);

    GFileBanks.Delete(idx);
    REQUIRE(GFileBanks.Size() == origSize);
}

TEST_CASE("AutoBank - no match returns nullptr", "[qstream][path][autobank]")
{
    REQUIRE(QIFStreamB::AutoBank("nonexistent/file.txt") == nullptr);
    REQUIRE(QIFStreamB::AutoBank("") == nullptr);
}

TEST_CASE("AutoBank - absolute path with drive letter returns nullptr", "[qstream][path][autobank]")
{
    REQUIRE(QIFStreamB::AutoBank("C:file.txt") == nullptr);
}

// QIFStream::open – file open with case-insensitive resolution on Linux

TEST_CASE("QIFStream open - exact path", "[qstream][path][fileopen]")
{
    REQUIRE_FIXTURE("qstream/test_input.txt");

    QIFStream stream;
    stream.open(GetTestFixturePath("qstream/test_input.txt"));

    REQUIRE(stream.fail() == false);
    REQUIRE(stream.rest() > 0);
}

#ifndef _WIN32
TEST_CASE("QIFStream open - backslash path resolved on Linux", "[qstream][path][fileopen][linux]")
{
    REQUIRE_FIXTURE("qstream/test_input.txt");

    // Build a path with backslashes (Windows-style)
    std::string exeDir = GetExecutableDirectory();
    std::string bsPath = exeDir + "/fixtures\\qstream\\test_input.txt";

    QIFStream stream;
    stream.open(bsPath.c_str());

    // LocalPath in OpenFileForRead converts \ to / before open
    REQUIRE(stream.fail() == false);
    REQUIRE(stream.rest() > 0);
}
#endif

TEST_CASE("QIFStream FileExists - exact path", "[qstream][path][fileexists]")
{
    REQUIRE_FIXTURE("qstream/test_input.txt");

    REQUIRE(QIFStream::FileExists(GetTestFixturePath("qstream/test_input.txt")) == true);
    REQUIRE(QIFStream::FileExists("nonexistent_file_12345.txt") == false);
}

TEST_CASE("QStream reads files from UTF-8 profile paths", "[qstream][path][fileexists][utf8]")
{
    const std::filesystem::path root = std::filesystem::temp_directory_path() /
#ifdef _WIN32
                                       std::filesystem::path(L"cwr_\u6d4b\u8bd5");
    const std::filesystem::path file = root / L"face.paa";
    const std::string rootUtf8 = Poseidon::WidePathToUtf8(root.wstring().c_str());
    const std::string fileUtf8 = Poseidon::WidePathToUtf8(file.wstring().c_str());
#else
                                       std::filesystem::path("cwr_\xE6\xB5\x8B\xE8\xAF\x95");
    const std::filesystem::path file = root / "face.paa";
    const std::string rootUtf8 = root.string();
    const std::string fileUtf8 = file.string();
#endif
    const char payload[] = "custom-face";

    REQUIRE(Poseidon::CreateDirectoryUtf8(rootUtf8.c_str()));
    REQUIRE(Poseidon::WriteFileUtf8(fileUtf8.c_str(), payload, sizeof(payload)));
    REQUIRE(QIFStream::FileExists(fileUtf8.c_str()));

    FileBufferLoaded buffer(fileUtf8.c_str());
    REQUIRE(buffer.GetSize() == sizeof(payload));
    CHECK(memcmp(buffer.GetData(), payload, sizeof(payload)) == 0);

    std::error_code ec;
    std::filesystem::remove_all(root, ec);
}

#ifndef _WIN32
TEST_CASE("QIFStream FileExists - case insensitive on Linux", "[qstream][path][fileexists][linux]")
{
    REQUIRE_FIXTURE("qstream/test_input.txt");

    // FilePathExists has CI fallback on Linux
    std::string exeDir = GetExecutableDirectory();

    // Try various case combats
    std::string mixed = exeDir + "/fixtures/QStream/Test_Input.txt";
    // CI resolution walks each component with strcasecmp
    REQUIRE(QIFStream::FileExists(mixed.c_str()) == true);
}

TEST_CASE("QIFStream FileExists - backslash path on Linux", "[qstream][path][fileexists][linux]")
{
    REQUIRE_FIXTURE("qstream/test_input.txt");

    std::string exeDir = GetExecutableDirectory();
    std::string bsPath = exeDir + "/fixtures\\qstream\\test_input.txt";

    REQUIRE(QIFStream::FileExists(bsPath.c_str()) == true);
}
#endif

// QIFStreamB::FileExist – bank + filesystem lookup

TEST_CASE("QIFStreamB FileExist - filesystem fallback", "[qstream][path][bankexist]")
{
    REQUIRE_FIXTURE("qstream/test_input.txt");

    bool exists = QIFStreamB::FileExist(GetTestFixturePath("qstream/test_input.txt"));
    REQUIRE(exists == true);

    REQUIRE(QIFStreamB::FileExist("nonexistent_xyz_99999.bin") == false);
}

// PBO bank loading with cross-platform path handling

TEST_CASE("PBO bank - load and lookup file", "[qstream][path][pbo]")
{
    REQUIRE_FIXTURE("pbo/mission_fixture.Intro.pbo");

    int origSize = GFileBanks.Size();
    int idx = GFileBanks.Add();
    QFBank& bank = GFileBanks.Set(idx);

    std::string pboPath = GetTestFixturePath("pbo/mission_fixture.Intro.pbo");
    bank.SetPrefix("mission_fixture.Intro\\");
    bank.Lock();

    // Open the PBO (Load happens via open -> Load -> ReadFileInfos)
    bool opened = bank.open(pboPath.c_str(), nullptr, nullptr);

    if (opened && !bank.error())
    {
        // The PBO contains "mission.sqm" as first entry
        // FindFileInfo lowercases the key, so mixed case matches
        REQUIRE(bank.FileExists("mission.sqm") == true);
        REQUIRE(bank.FileExists("MISSION.SQM") == true);
        REQUIRE(bank.FileExists("Mission.Sqm") == true);
        REQUIRE(bank.FileExists("nonexistent.txt") == false);
    }

    GFileBanks.Delete(idx);
    REQUIRE(GFileBanks.Size() == origSize);
}

TEST_CASE("PBO bank - lookup normalizes slashes", "[qstream][path][pbo]")
{
    REQUIRE_FIXTURE("pbo/addon_fixture.pbo");

    int origSize = GFileBanks.Size();
    int idx = GFileBanks.Add();
    QFBank& bank = GFileBanks.Set(idx);

    std::string pboPath = GetTestFixturePath("pbo/addon_fixture.pbo");
    bank.SetPrefix("addon_fixture\\");
    bank.Lock();

    bool opened = bank.open(pboPath.c_str(), nullptr, nullptr);

    if (opened && !bank.error())
    {
        // addon_fixture.pbo contains "config.bin" and "stringtable.csv"
        REQUIRE(bank.FileExists("config.bin") == true);
        REQUIRE(bank.FileExists("stringtable.csv") == true);
    }

    GFileBanks.Delete(idx);
    REQUIRE(GFileBanks.Size() == origSize);
}

// AutoOpen with bank – full pipeline path resolution

TEST_CASE("AutoOpen - opens file from PBO bank", "[qstream][path][autoopen]")
{
    REQUIRE_FIXTURE("pbo/mission_fixture.Intro.pbo");

    int origSize = GFileBanks.Size();
    bool origUseBanks = GUseFileBanks;
    GUseFileBanks = true;

    int idx = GFileBanks.Add();
    QFBank& bank = GFileBanks.Set(idx);

    std::string pboPath = GetTestFixturePath("pbo/mission_fixture.Intro.pbo");
    bank.SetPrefix("mission_fixture.Intro\\");
    bank.Lock();

    bool opened = bank.open(pboPath.c_str(), nullptr, nullptr);

    if (opened && !bank.error())
    {
#ifdef _WIN32
        const char* query = "mission_fixture.Intro\\mission.sqm";
#else
        const char* query = "mission_fixture.Intro/mission.sqm";
#endif

        QIFStreamB stream;
        stream.AutoOpen(query);

        REQUIRE(stream.fail() == false);
        REQUIRE(stream.rest() > 0);
    }

    GFileBanks.Delete(idx);
    GUseFileBanks = origUseBanks;
    REQUIRE(GFileBanks.Size() == origSize);
}

TEST_CASE("AutoOpen - cross-slash bank lookup", "[qstream][path][autoopen]")
{
    REQUIRE_FIXTURE("pbo/mission_fixture.Intro.pbo");

    int origSize = GFileBanks.Size();
    bool origUseBanks = GUseFileBanks;
    GUseFileBanks = true;

    int idx = GFileBanks.Add();
    QFBank& bank = GFileBanks.Set(idx);

    std::string pboPath = GetTestFixturePath("pbo/mission_fixture.Intro.pbo");
    bank.SetPrefix("mission_fixture.Intro\\");
    bank.Lock();

    bool opened = bank.open(pboPath.c_str(), nullptr, nullptr);

    if (opened && !bank.error())
    {
        // Query with forward slash should match bank with backslash prefix
        // (CmpStartStr treats / and \ as equivalent)
        QIFStreamB stream;
        stream.AutoOpen("mission_fixture.Intro/mission.sqm");

        REQUIRE(stream.fail() == false);
        REQUIRE(stream.rest() > 0);
    }

    GFileBanks.Delete(idx);
    GUseFileBanks = origUseBanks;
    REQUIRE(GFileBanks.Size() == origSize);
}

TEST_CASE("AutoOpen - case insensitive bank prefix", "[qstream][path][autoopen]")
{
    REQUIRE_FIXTURE("pbo/mission_fixture.Intro.pbo");

    int origSize = GFileBanks.Size();
    bool origUseBanks = GUseFileBanks;
    GUseFileBanks = true;

    int idx = GFileBanks.Add();
    QFBank& bank = GFileBanks.Set(idx);

    std::string pboPath = GetTestFixturePath("pbo/mission_fixture.Intro.pbo");
    bank.SetPrefix("mission_fixture.Intro\\");
    bank.Lock();

    bool opened = bank.open(pboPath.c_str(), nullptr, nullptr);

    if (opened && !bank.error())
    {
        // Mixed case query should still find the bank
        QIFStreamB stream;
        stream.AutoOpen("ABC.INTRO/mission.sqm");

        REQUIRE(stream.fail() == false);
        REQUIRE(stream.rest() > 0);
    }

    GFileBanks.Delete(idx);
    GUseFileBanks = origUseBanks;
    REQUIRE(GFileBanks.Size() == origSize);
}

// Edge cases

TEST_CASE("CmpStartStr - single char separators", "[qstream][path][cmpstart][edge]")
{
    REQUIRE(CmpStartStr("/", "\\") == 0);
    REQUIRE(CmpStartStr("\\", "/") == 0);
    REQUIRE(CmpStartStr("/", "/") == 0);
    REQUIRE(CmpStartStr("\\", "\\") == 0);
}

TEST_CASE("CmpStartStr - adjacent separators", "[qstream][path][cmpstart][edge]")
{
    REQUIRE(CmpStartStr("a//b", "a\\\\b") == 0);
    REQUIRE(CmpStartStr("a\\/b", "a/\\b") == 0);
}

TEST_CASE("QFBank SetPrefix - empty prefix", "[qstream][path][bank][edge]")
{
    QFBank bank;
    bank.SetPrefix("");
    REQUIRE(bank.GetPrefix().GetLength() == 0);
}

TEST_CASE("QFBank SetPrefix - no slash in prefix", "[qstream][path][bank][edge]")
{
    QFBank bank;
    bank.SetPrefix("plain");
    REQUIRE(strcmp(bank.GetPrefix().Data(), "plain") == 0);
}

// A path naming its mod folder through a parent escape resolves into that mod.
namespace
{
void PutBankInt(std::ofstream& out, int32_t v)
{
    out.write(reinterpret_cast<const char*>(&v), sizeof(v));
}

// PBO entry tail, in file order: mime, original size, offset, timestamp, data length.
void PutBankEntry(std::ofstream& out, const std::string& name, int32_t offset, int32_t length)
{
    out.write(name.c_str(), static_cast<std::streamsize>(name.size()) + 1);
    PutBankInt(out, 0);
    PutBankInt(out, 0);
    PutBankInt(out, offset);
    PutBankInt(out, 0);
    PutBankInt(out, length);
}

// One uncompressed member, OFP header shape (no property block, no trailer). Local
// rather than a fixture: the committed pbo/ family is absent in a source checkout, and
// a test that pins the bank namespace must not skip for that reason.
bool WriteSingleMemberPbo(const std::filesystem::path& path, const std::string& member, const std::string& data)
{
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    if (!out.good())
        return false;
    PutBankEntry(out, member, 0, static_cast<int32_t>(data.size()));
    PutBankEntry(out, "", 0, 0); // terminator
    out.write(data.data(), static_cast<std::streamsize>(data.size()));
    out.close();
    return out.good() || std::filesystem::exists(path);
}
} // namespace

// The addon-bank fallback (NormalizeAddonBankPath) collapses a root-relative name and
// indexes the bank table with the result. That is only sound while the collapsed name
// still names something INSIDE the game root: a ".." that walks above its own start
// leaves it, and must not be collapsed into a bank lookup. The positive rows are the
// documented purpose of the fallback (a path that reaches addons/ the long way round);
// the negative rows are the QSTREAM-ESCAPE-1 rule applied to the bank namespace.
TEST_CASE("QIFStreamB FileExist - bank path names never climb above the game root",
          "[qstream][path][bankexist][bankescape]")
{
    const int origSize = GFileBanks.Size();
    const bool origUseBanks = GUseFileBanks;
    GUseFileBanks = true;

    const std::filesystem::path pboPath =
        std::filesystem::temp_directory_path() / "qstream_bankescape_addon_fixture.pbo";
    REQUIRE(WriteSingleMemberPbo(pboPath, "stringtable.csv", "key,value\n"));

    const int idx = GFileBanks.Add();
    QFBank& bank = GFileBanks.Set(idx);
    bank.SetPrefix("addon_fixture\\");
    bank.Lock();
    std::string bankName = pboPath.string();
    bankName.resize(bankName.size() - 4); // QFBank::open appends ".pbo"
    REQUIRE(bank.open(bankName.c_str(), nullptr, nullptr));
    REQUIRE_FALSE(bank.error());
    REQUIRE(bank.FileExists("stringtable.csv"));

    // By its bank prefix.
    CHECK(QIFStreamB::FileExist("addon_fixture\\stringtable.csv"));

    // The fallback's reason for existing: a root-relative name that collapses to
    // addons/<bank>/<member> still names the bank member.
    CHECK(QIFStreamB::FileExist("anims\\..\\addons\\addon_fixture\\stringtable.csv"));
    CHECK(QIFStreamB::FileExist("addons\\..\\addons\\addon_fixture\\stringtable.csv"));

    // Above the root: no collapse may turn these into a bank lookup.
    CHECK_FALSE(QIFStreamB::FileExist("..\\addons\\addon_fixture\\stringtable.csv"));
    CHECK_FALSE(QIFStreamB::FileExist("x\\..\\..\\addons\\addon_fixture\\stringtable.csv"));
    CHECK_FALSE(QIFStreamB::FileExist("..\\addon_fixture\\stringtable.csv"));

    // The open path shares the fallback, so it must agree with the existence check.
    {
        QIFStreamB stream;
        stream.AutoOpen("anims\\..\\addons\\addon_fixture\\stringtable.csv");
        CHECK_FALSE(stream.fail());
        CHECK(stream.rest() > 0);
    }
    {
        QIFStreamB stream;
        stream.AutoOpen("..\\addons\\addon_fixture\\stringtable.csv");
        CHECK(stream.fail());
    }

    GFileBanks.Delete(idx);
    GUseFileBanks = origUseBanks;
    std::error_code ec;
    std::filesystem::remove(pboPath, ec);
    REQUIRE(GFileBanks.Size() == origSize);
}

TEST_CASE("QIFStreamB FileExist - mod path escaping its prefix", "[qstream][path][bankexist][mod]")
{
    const std::filesystem::path root = std::filesystem::temp_directory_path() / "cwr_modescape";
    const std::filesystem::path voiceDir = root / "@voicemod" / "voice" / "Jari";
    const char payload[] = "wss";

    std::error_code ec;
    std::filesystem::remove_all(root, ec);
    REQUIRE(std::filesystem::create_directories(voiceDir, ec));
    REQUIRE(Poseidon::WriteFileUtf8((voiceDir / "reportstatus.wss").string().c_str(), payload, sizeof(payload)));

    Poseidon::ModSystem::SetModPath((root / "@voicemod").string().c_str());

    CHECK(QIFStreamB::FileExist("@voicemod\\voice\\Jari\\reportstatus.wss"));
    CHECK(QIFStreamB::FileExist("voice\\..\\@voicemod\\voice\\Jari\\reportstatus.wss"));
    CHECK_FALSE(QIFStreamB::FileExist("voice\\..\\@voicemod\\voice\\Adam\\reportstatus.wss"));
    CHECK_FALSE(QIFStreamB::FileExist("..\\@voicemod\\voice\\Jari\\reportstatus.wss"));

    Poseidon::ModSystem::SetModPath("");
    std::filesystem::remove_all(root, ec);
}
