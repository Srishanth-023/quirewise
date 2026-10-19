# Component signing key

`components.pub` holds the minisign public key (the base64 line of a `.pub` file) that every
component list and offline package must be signed with. It is empty on purpose: a build with no key
installs nothing, so no one can ship components that were signed by a stranger.

To release:

1. Create a key pair once, on a machine you trust: `minisign -G -p components.pub -s components.key`.
2. Keep `components.key` offline. Never commit it.
3. Put the second line of `components.pub` (the base64 text, without the comment line) in this file.
4. Sign `components.json` with `minisign -S -s components.key -m components.json` and publish both
   `components.json` and `components.json.minisig` at `DEFAULT_MANIFEST_URL`.
