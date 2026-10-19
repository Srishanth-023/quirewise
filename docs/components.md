# Components

Some tools need a program that is too large or too special to ship inside the installer. Quirewise
installs these on demand, with the person's consent, and checks every download before it is used
(ADR-0005).

## What can be installed

| Id | Used by | Notes |
|---|---|---|
| `libreoffice` | Word, PowerPoint and Excel to PDF when Microsoft Office is not installed | A copy of LibreOffice already installed on the computer is used too, so nothing is downloaded for it |
| `tesseract` | OCR, Scan to PDF with OCR | `tesseract.exe` and its libraries in one folder, per architecture (`scripts/build-tesseract.ps1`) |
| `tessdata-<code>` | The same | One language, a `<code>.traineddata` file (tessdata_fast); `chi_sim` is `tessdata-chi-sim`. `scripts/fetch-tessdata.ps1` gets them for development |

More components (PDF/A validation, local AI) arrive with their tools.

Every OCR run builds a private data folder with links to the language files it needs, so languages from different components can be combined; the folder and its links are removed afterwards. In a development tree the program is found in `vendor\tesseract\<arch>` and the languages in its `tessdata` folder.

## Where things live

- Installed components: `%LOCALAPPDATA%\Quirewise\components\<id>\<version>\` (portable mode: `data\components`).
  Each version folder holds a `component.json` record that is written last, so a half-finished
  install is never listed.
- Downloads wait in `components\.download` while they are checked and are removed afterwards.

## How a download is checked

1. The component list `components.json` and its signature `components.json.minisig` are fetched
   over HTTPS (Windows checks the certificate).
2. The signature is checked against the minisign (Ed25519) public key in
   `crates/infrastructure/trust/components.pub`. A build without a key installs nothing.
3. The list is parsed with strict rules (size, schema, https only, no duplicates).
4. The payload is streamed to disk while its SHA-256 and size are computed. Both must match the signed
   list, or the file is deleted and `QW-COMPONENT-VERIFY-FAILED` is reported.
5. The zip is unpacked into `<version>.partial`: no path may leave the folder, links are refused, and
   file count and size are limited. The folder is renamed into place only when everything worked, and
   older versions of the same component are removed.

## Offline packages

A `.qwpkg` file is a zip with three entries: `entry.json` (the list entry for the component),
`entry.json.minisig` (its signature) and `payload.zip`. Importing checks the signature, the size and
the SHA-256, then installs exactly like a download. Use it on computers without internet.

## Using them

- Settings, Components: see what is installed, check what is offered (asks before using the internet),
  install, import a package, remove.
- Command line: `qw components list`, `qw components available --yes`, `qw components install <id> --yes`,
  `qw components import <file>`, `qw components remove <id>`.

## Release checklist for the owner

1. Create the signing key pair (see `crates/infrastructure/trust/README.md`) and put the public key in
   `trust/components.pub`.
2. Host `components.json` and its `.minisig` and set `DEFAULT_MANIFEST_URL` in
   `crates/infrastructure/src/components.rs`.
3. Build the component zips (LibreOffice portable, per architecture) and list them with their size
   and SHA-256.
