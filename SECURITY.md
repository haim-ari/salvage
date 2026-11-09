# Security

## Report a problem

Do not open a public issue for a security problem.

Use [Report a vulnerability](https://github.com/haim-ari/salvage/security/advisories/new) on GitHub. Only the maintainer can see it.

Please tell us:

- what the problem is,
- how to make it happen,
- which version you use.

We answer within 7 days.

## Supported versions

Only the latest release gets fixes.

## How Salvage keeps your data safe

- You type your Apple Account password into Apple's own sign-in page. Salvage never sees or saves it.
- The sign-in session stays in the macOS web view cookie store, like in Safari.
- Salvage talks only to Apple's iCloud servers, over HTTPS.
- The saved scan is in a folder that only your macOS user can read.
- The window can call only Salvage's own commands. A strict content policy blocks outside scripts.
- "Show in Finder" opens only folders that Salvage restored files to.
