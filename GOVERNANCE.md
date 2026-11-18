# Governance

Rustic Maps SDK is maintained by the people listed in
[`MAINTAINERS.md`](MAINTAINERS.md). This page states how decisions are made and
how the maintainer group changes.

## Maintainer roles

- **Area maintainers** review and merge changes in one area: core and renderer,
  providers, Android, iOS, React Native, WebAssembly, or documentation and
  release.
- **Release maintainers** cut releases from `main`, verify the signing and the
  published artifacts, and record the release notes.
- **Repository administrators** manage settings, access, and the maintainer
  list. The repository settings hold the authoritative access list.

One person may hold more than one role.

## Decisions

Maintainers use lazy consensus on issues and pull requests. A proposal proceeds
when no maintainer objects during the review window stated on the issue or pull
request. An objection moves the proposal to discussion; it does not count as a
rejection.

A decision that changes the public API, the frozen contract, the supported
platform set, or the license is a design decision. It needs a maintainer
ruling, and the ruling is recorded in the roadmap or the architecture documents
in this repository. The public record is the issue or pull request thread that
contains the ruling.

## Releases

A release maintainer has release authority. Releases are cut from `main` after
the release gate passes, and a published release is immutable. A defect in a
release is fixed in a new release, never by rewriting the published artifact.

## Adding maintainers

A contributor becomes a maintainer after sustained, high-quality contributions
to an area and a nomination by an existing maintainer. The existing maintainers
decide by lazy consensus. The new role and area are recorded in
[`MAINTAINERS.md`](MAINTAINERS.md) and in the repository access settings.

A maintainer who becomes inactive for a long period moves to emeritus status.
The change follows the same consensus process.

## Versioning and compatibility

The project is pre-1.0. A minor release may add to the public API and the frozen
contract. A breaking change requires a maintainer ruling, and it ships with a
documented deprecation window, a `CHANGELOG.md` entry, and updated bindings and
documentation in the same change. The published versions page states the
support rules for each release line.
