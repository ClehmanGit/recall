# Security

## Reporting a problem

Please report security problems privately, not in a public issue. Use **[Report a vulnerability](https://github.com/ClehmanGit/recall/security/advisories/new)** on the repository's Security tab.

Include the Recall version (shown at the bottom of Settings), your Windows version, and the steps to reproduce. You should get a reply within a week. Fixes ship as a new release, and the advisory is published once the release is out.

## Supported versions

Only the latest release gets security fixes. Recall does not update itself yet, so download the newest installer from the [releases page](https://github.com/ClehmanGit/recall/releases/latest).

## In scope

Anything that breaks one of these promises:

- Recall sends nothing off the computer (except the optional feedback box, which official releases leave out).
- Recall reads only the folders the person added and the chat apps they signed in to.
- Recall never sees the passwords typed into the chat apps' sign-in pages.
- A web page shown in a sign-in window can't read files, call the app's other commands, or act for another chat app.
- Recall never moves, changes, deletes or runs the person's files.

Also in scope: a document that makes Recall run code, or crash every time it starts.

## Out of scope

- Someone who already has access to the Windows account. The index on disk is not encrypted; it is as private as the account it is stored in.
- The chat apps' own websites and sign-in pages.
- The SmartScreen warning shown because the installer is not code-signed.

How the app enforces these limits is described in [How Recall works](docs/how-it-works.md#security-model).
