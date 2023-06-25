For integrating the C/C++ GameActivity glue code with `android-activity` and
Rust we need to make a number of changes to the C/C++ code that comes from
Google's agdk-libraries releases.

# Patches

To help track the changes we've made we add a copy of each patch applied to this
directory.

When we come to update to a new AGDK release we need to review each of these
patches to determine whether they are still needed (some of them are bug fixes
or fill in some missing features) and if so we can port that change across and
create a new patch.

Each commit that modifies Google's upstream GameActivity code should start with
"GameActivity PATCH:" to also help with tracking them down via the Git log.

# Other Modifications

There are a few C symbols that need to be exported from the cdylib that's built
for GameActivity to load at runtime but Rust/Cargo doesn't support compiling
C/C++ code in a way that can export these symbols directly and we instead have
to export wrappers from Rust code.

At the bottom of GameActivity.cpp then
`Java_com_google_androidgamesdk_GameActivity_loadNativeCode` should be given a
`_C` suffix like `Java_com_google_androidgamesdk_GameActivity_loadNativeCode_C`

At the bottom of `android_native_app_glue.c` and `android_native_app_glue.h`
`GameActivity_onCreate` should also be given a `_C` suffix like
`GameActivity_onCreate_C`

Since we want to call the application's main function from Rust after
initializing our own `AndroidApp` state, but we want to let applications use the
same `android_main` symbol name then `android_main` should be renamed to
`_rust_glue_entry` in `android_native_app_glue.h` and
`android_native_app_glue.c`

# Synchronizing with Upstream

Upstream distribute `android_native_app_glue.c` and `GameActivity.cpp` code as a
"prefab" that is bundled as part of a `GameActivity-release.aar` archive. The
idea is that it's a build system agnostic way of bundling native glue code with
archives that build systems can extract the code via a command line tool, along
with some metadata to describe how it should be compiled.

It's fairly easy to extract the C/C++ files and just integrate them in a way
that suits Rust / Cargo better.

`.aar` files are simply zip archives that can be unpacked and the files under
`prefab/modules/game-activity/include` can be moved to `csrc/` in this repo,
which will then be built by `build.rs` via the `cc` crate.

The easiest way I found to get to the `GameActivity-release.aar` is to download
the "express" agdk-libraries release from
https://developer.android.com/games/agdk/download, and you should find
`GameActivity-release.aar` at the top level of the archive after unpacking.

The git repo for the source code can be found here:
https://android.googlesource.com/platform/frameworks/opt/gamesdk/ with the
prefab code under `GameActivity/prefab-src/modules/game-activity/include` -
though it may be best to synchronize with official releases.

# Current Version

The current version of GameActivity glue code is from:
https://android.googlesource.com/platform/frameworks/opt/gamesdk/  commit = e8c66318443e5c864395725d7e4416d5b46242f8

This is from May 25 2022 (corresponding to AGDK 2022.0.0)

# Breaking changes?

We only want to require a backwards-incompatible version bump for
`android-activity` for semver breaking changes - either in the Rust API or in
the Java APIs for `NativeActivity` and `GameActivity`.

Awkwardly AGDK GameActivity releases don't follow semver and so we have to audit
the changes manually to determine whether there has been a semver breaking change
in the Java APIs.