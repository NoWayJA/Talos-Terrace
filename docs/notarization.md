> Adapted from Modz (NoWayJA/Modz, MIT); the original signing runbooks were lifted on 17 September 2026.

# Signing on a fresh Mac

1. Install Xcode command line tools and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).
2. Install or create a Developer ID Application identity in Keychain Access. The matching
   private key is required; downloading the certificate alone does not recover a lost key.
3. Keep system trust defaults and restrict key access to the signing tool.
4. Follow [signing.md](signing.md) to build, save notarization credentials in a Keychain
   profile, submit, staple and verify.
5. Test the final downloaded archive on a clean Mac with Gatekeeper enabled.

No credentials belong in this guide, the repository, `.zshrc` or shared output. Keep any
exported signing identity in protected storage outside the clone. The first signed release
has not yet been validated; passing a local development build is not that validation.
