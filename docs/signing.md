> Adapted from Modz (NoWayJA/Modz, MIT); the original signing runbooks were lifted on 17 September 2026.

# macOS signing and notarization

Talos Terrace has no published binaries yet. This is the release procedure to verify on the first
release, not a claim that signing has already been tested. Follow the current
[Tauri signing guide](https://tauri.app/distribute/sign/macos/) and
[Apple notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).

## Prepare the signing identity

Use an Apple Developer membership and a Developer ID Application certificate with its matching
private key in Keychain Access. Create the certificate request there so the key stays in the
keychain. Keep certificate trust at system defaults and approve access for `codesign` when
macOS asks. Do not grant every application access to the private key or change access rules
for the entire login keychain.

```sh
security find-identity -v -p codesigning
```

Set the non-secret signing identity for the build session:

```sh
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
npm run tauri:build -- --bundles app
```

This creates `src-tauri/target/release/bundle/macos/Talos Terrace.app`. The product name
contains a space, so every path to the bundle below is quoted. The identity is supplied by
the environment; the repository does not pin a maintainer's certificate. No notarization
credential is needed for this signing-only build. Do not set Tauri's automatic-notarization
credential variables when following the manual workflow below.

## Notarize with a Keychain profile

Store notarization credentials interactively. The command prompts for the app-specific
password; keep it out of shell history, shell startup files, logs and the repository.
[Apple documents Keychain profiles](https://developer.apple.com/documentation/technotes/tn3147-migrating-to-the-latest-notarization-tool).

```sh
xcrun notarytool store-credentials terrace-notary --apple-id YOUR_APPLE_ID --team-id YOUR_TEAM_ID
ditto -c -k --keepParent "src-tauri/target/release/bundle/macos/Talos Terrace.app" /tmp/talos-terrace-submit.zip
xcrun notarytool submit /tmp/talos-terrace-submit.zip --keychain-profile terrace-notary --wait
```

Continue only after an **Accepted** result. Staple the ticket, validate the app, then create
the distributable ZIP from the stapled app:

```sh
xcrun stapler staple "src-tauri/target/release/bundle/macos/Talos Terrace.app"
xcrun stapler validate "src-tauri/target/release/bundle/macos/Talos Terrace.app"
codesign --verify --deep --strict --verbose=2 "src-tauri/target/release/bundle/macos/Talos Terrace.app"
spctl --assess --type execute --verbose=2 "src-tauri/target/release/bundle/macos/Talos Terrace.app"
ditto -c -k --keepParent "src-tauri/target/release/bundle/macos/Talos Terrace.app" /tmp/talos-terrace-notarized.zip
```

Test the final download on a clean Mac, with quarantine intact, before distributing it.
Signing, notarization and a checksum serve different purposes; include the final archive's
SHA-256 checksum with the release. Intel and Apple silicon builds need separate verification.

## Troubleshooting

If signing fails, inspect the matching private key, certificate expiry and Apple's trust
chain. If notarization fails, use `notarytool log` with the submission ID and the same
Keychain profile. Never print the password to diagnose authentication.

A Gatekeeper warning is a failed release check. Verify the signature and notarization of the
exact downloaded artifact; do not tell recipients to clear quarantine or disable Gatekeeper.
CI signing, Windows signing, Linux packaging and SBOM publication still need release work.
