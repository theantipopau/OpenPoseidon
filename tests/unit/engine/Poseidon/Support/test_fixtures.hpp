#pragma once

// One definition only. This file used to be a second, divergent copy of the fixture
// helpers: same macro names, no missing-file handling, no repo-walk resolution. A
// test's behaviour then depended on which spelling of the include path it used.
// Forward to the single implementation so Support/ and the rest of the suite agree.
#include "../test_fixtures.hpp"
