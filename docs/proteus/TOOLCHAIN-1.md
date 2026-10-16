# TOOLCHAIN-1 — VS-bundled LLVM candidates in FindLLVMSanitizer.cmake

## Symptom (BACKEND-VERIFY-1 follow-up)

`find_llvm_root` knew only `C:/D:/Program Files/LLVM`. On boxes without a
standalone LLVM install (VS-bundled LLVM only), fresh configures fail on
LLVM-not-found; the workaround was `LLVM_ROOT=<VS2022>/VC/Tools/Llvm/x64` plus
`VCPKG_KEEP_ENV_VARS=LLVM_ROOT` (env-only, unrecorded).

## Fix (`cmake/FindLLVMSanitizer.cmake`, x64 candidates only)

Added the VS 2022-bundled LLVM paths as candidates (Community, Professional,
Enterprise, BuildTools). The `LLVM_ROOT` env override still wins (checked first),
so this only changes the fallback. x86 and Linux paths untouched.

## Verification

- Full `win-x64-clang-rwdi` configure ran green with this file changed (cmake
  re-ran configure on edit during the `Poseidon` build; build exit 0).
- Behaviour on boxes with standalone LLVM is unchanged (earlier candidates win).
- Not verified: a from-scratch configure on a box with VS-bundled LLVM only and
  no `LLVM_ROOT` set — that is the exact situation this fixes, still owed.

## Follow-ups

- None code-wise. If the owed verification fails, the candidate list needs the
  actual install path from that box.
