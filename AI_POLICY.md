# AI usage policy

AI tools may be used to write code, tests, and documentation for this project.
The policy is short and applies to every contribution.

## Responsibility stays with the human

The person who opens a pull request or signs off a commit is responsible for
every line in it: correctness, tests, security, license compatibility, and
behavior on the supported platforms. A tool cannot sign the Developer
Certificate of Origin and cannot take responsibility for the result.

## Disclose material assistance

Disclose material AI assistance in the pull request description. State which
tool was used and which parts of the change it produced. Routine assistance such
as completion, formatting, or a small refactor needs a one-line disclosure. A
change produced largely by a tool needs a fuller description of what was
reviewed and tested.

Disclosure is not a rejection. It lets a reviewer ask the right questions.

## Never paste secrets

Never give an AI tool a provider credential, a signing key, a token, a private
key, or a private log. Never let a generated change add a credential, a vendor
secret, or a personal path to the repository. The credential rules are in
[`SECURITY.md`](SECURITY.md).

## Provenance on request

A reviewer may ask for the prompt, the tool, or the reasoning behind a change.
Answer in the pull request thread. If the provenance cannot be established, the
maintainer may ask for the change to be rewritten or resubmitted.

## What this policy does not do

It does not ban AI tools. It does not change the contribution license, the DCO
requirement, the test requirement, or the review standard. See
[`CONTRIBUTING.md`](CONTRIBUTING.md) for those rules.
