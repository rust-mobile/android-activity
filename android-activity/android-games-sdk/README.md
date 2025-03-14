# android-games-sdk-rs

This is a grafted version of [android-games-sdk-rs][agdk].

The purpose of android-games-sdk-rs is to track our patches on top of the Android Game Development Kit (AGDK) so that it is easier to update to new upstream versions.

## Updating to new agdk version checklist

This is a basic checklist for things that need to be done before updating to a new agdk version:

- [ ] Ensure all patches applied to the local repo have been backported (see section below on backporting patches)
- [ ] Rebase patches on top of a new *release* version of agdk. If you're not sure which version you should be rebasing on, open an issue.
- [ ] If there have been substantial path changes, remove the existing files first so that its a clean graft
- [ ] Copy any new files over from the new version using the [copy_files](./copy_files) script (see section below on copying files)
- [ ] Update [build.rs](../build.rs) with any new includes and src files
- [ ] Regenerate ffi bindings using [generate_bindings.sh](./generate_bindings.sh)

## Backporting patches

Changes made to these files must be backported to [android-games-sdk-rs][agdk], otherwise they will be lost when updating to newer upstream versions. This can be done like so (running from the project root):

```bash
git format-patch -o ~/agdk-patches last-import.. -- android-activity/android-games-sdk
```

When applying on the [android-games-sdk-rs][agdk] side:

```bash
git checkout android-activity-4.0.0
git am -p3 ~/agdk-patches/*.patch
```

Once these are applied on top of the current imported agdk version (in this example 4.0.0). Then they can be rebased onto future agdk versions easily.

## Copying files from agdk

When updating to a new version of upstream agdk properly, you can use the [copy_files](./copy_files) script.

This script takes in a list of files from [file_list.txt](./file_list.txt), and grafts those files into the local directory. This ensures that pathes are aligned so that patching is easier.

[agdk]: https://github.com/rust-mobile/android-games-sdk-rs
