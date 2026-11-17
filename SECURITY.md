# Security policy

## Reporting a vulnerability

Please report a security problem in Stet privately, through GitHub's private vulnerability reporting: **[Report a vulnerability](https://github.com/PixDevsApps/Stet/security/advisories/new)** (also under the repository's Security tab). Don't open a public issue, pull request or discussion for it.

A report helps most when it says:

- which version of Stet (`stet --version`) and how it was installed (Arch package, Flatpak or from source);
- what an attacker needs (a file you open, a project folder you search, a place on the network path) and what they get;
- how to reproduce it, or which code you read, and what you saw;
- whether you have told anyone else, such as the maintainers of a library Stet uses.

Please put everything in the report as text, and never attach files with private text in them.

No GitHub account? Write to **support@pixdevs.com** with "Security" in the subject and only a short description, and the maintainer answers with a private way to send the details.

## What happens next

The report stays private between you and the maintainer, and the answer comes in the report's own thread. Once the problem is confirmed, the fix comes in a release, and a GitHub security advisory is published with it, crediting you if you want. When the fault is in a library Stet uses, its maintainers are told privately before the fix is published.

## Supported versions

Security fixes go into the latest release only. **Check for Updates…** in Stet's main menu says whether you have it, and the [user guide](docs/USER_GUIDE.md#updating) shows how to update.

## Scope

Stet's own code in this repository, its release package and its Flatpak repository. A problem in another library alone (GTK, libadwaita, GtkSourceView, PCRE2) belongs with that project; tell us as well when the way Stet uses it makes it worse.
