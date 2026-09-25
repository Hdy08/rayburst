# Code signing

Rayburst keeps the existing Tauri updater signing key. The public key is checked in
under `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`. Private material stays
in `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository
secrets. Rebranding does not require key rotation.

Release builds enable updater signatures through `src-tauri/tauri.release.json`.
Local builds do not require a signing key unless that release configuration is used.

Tauri updater signatures verify downloaded update packages. Windows Authenticode
signing is a separate, opt-in SignPath operation. It uses the existing `SIGNPATH_*`
configuration and signs the final installers before their updater signatures and
channel manifests are published. See [Release configuration](RELEASING.md).
