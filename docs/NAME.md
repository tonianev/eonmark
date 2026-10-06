# The name

This document records why the project is called Eonmark, which availability and trademark checks were done, what they found, the runner-up names in case a conflict ever appears, and the rule that no placeholder crates are published. The only places the inspiration's title appears are [design/prior-art.md](design/prior-art.md) and the one disclaimed sentence in [README.md](../README.md); this file does not name it.

## Why Eonmark

"Eon" for the ages a match moves through. "Mark" for a borderland, as in Denmark or the medieval marches. The two signature systems of the game, tech-gated ages and territory borders, in one calm word. It is short, pronounceable, spelled as it sounds, and no software product, game or trademark using it was found in the checks below.

Where it appears: the repository `tonianev/eonmark`, the binary `eonmark`, the bundle identifier `com.tonianev.eonmark`, the window title, the `.app` name and the GitHub topics. The workspace crates are named `sim`, `rules`, `ai`, `game` and `sim-cli`; none of them is published.

## Checks

First pass 2026-10-05 (planning), full pass 2026-10-06 (M0 close). Register searches were run in a browser and the result pages saved under [name/](name/). Command-line checks list the exact command so they can be re-run.

### Trademark registers (2026-10-06)

| Register | Query | Result | Evidence |
|---|---|---|---|
| USPTO Trademark Search (tmsearch.uspto.gov), all classes incl. 9 and 41 | Wordmark contains `eonmark` | No results found. Live 0, Dead 0. | [name/uspto-eonmark-2026-10-06.jpg](name/uspto-eonmark-2026-10-06.jpg) |
| USPTO Trademark Search, all classes | Wordmark contains `eon mark` | No results found. | same session, not saved |
| EUIPO eSearch plus | `eonmark` (contains) | 1 result: EUTM 001862671 `EYEONMARKET`, word mark, Intel Corporation, Nice classes 35, 38, 42, status "Application withdrawn", last publication 2015. A substring hit in unrelated classes; not a conflict. Designs 0, Owners 0, Representatives 0. | [name/euipo-eonmark-2026-10-06.jpg](name/euipo-eonmark-2026-10-06.jpg) |
| WIPO Global Brand Database (branddb.wipo.int) | Brand name contains `eonmark` | No results found (76.9 million records, 89 sources). | [name/wipo-eonmark-2026-10-06.jpg](name/wipo-eonmark-2026-10-06.jpg) |

No live or dead mark in class 9 (software, downloadable games) or class 41 (entertainment services, online games) anywhere. The stop rule did not fire.

### Platforms and registries

| Where | Command or query | Result 2026-10-05 | Result 2026-10-06 |
|---|---|---|---|
| crates.io | `curl -s -o /dev/null -w '%{http_code}' https://crates.io/api/v1/crates/eonmark` | 404 | 404 (also `eonmark-sim` 404, `eonmark-rules` 404) |
| Steam store | `https://store.steampowered.com/search/?term=eonmark` | 0 results | 0 result rows; JSON endpoint `items: []` |
| itch.io search | `https://itch.io/search?q=eonmark` | 0 results | 0 games named eonmark (two unrelated fuzzy hits) |
| itch.io slug | `curl -s -o /dev/null -w '%{http_code}' https://eonmark.itch.io` | not checked | 404, free (`itch.io/profile/eonmark` 404) |
| GitHub repository search | `gh api 'search/repositories?q=eonmark+in:name'` | only an unrelated `eonmarket` repository | `tonianev/eonmark` (this project) and the same unrelated `eonmarket-catalogue` |
| GitHub user or organization | `gh api users/eonmark`, `gh api orgs/eonmark` | free | 404, 404, free |
| GitHub repository `tonianev/eonmark` | `gh repo view` | free | created 2026-10-05, public |

### Domains (2026-10-06, registry RDAP via rdap.org, cross-checked with whois)

| Domain | Result | Notes |
|---|---|---|
| `eonmark.com` | Registered | Registered 2026-04-17 by DropCatch.com 716 LLC, parked and listed for sale through HugeDomains, expires 2027-04-17. A drop-catch registration, which suggests the name had an earlier, expired owner. Not used by any product. |
| `eonmark.dev` | Not registered | Google Registry RDAP 404 |
| `eonmark.games` | Not registered | Identity Digital RDAP 404 |
| `eonmark.io` | Not registered | whois.nic.io: not found |

Recommendation: use `eonmark.dev` as the project domain and register it before the v0.1.0 announcement (M8). `eonmark.com` is parked by a reseller and is not worth buying for an open-source game; a prominent `.dev` plus the GitHub repository is enough for discoverability. Registering a domain needs the owner's account and payment, so it is an owner task listed in [ROADMAP.md](ROADMAP.md) under M8.

### Remaining for the owner

| Task | When | Why |
|---|---|---|
| Register `eonmark.dev` (or `.games`) | Before M8 | Name protection comes from the repository, the domain and the itch.io page |
| Re-run the four register and platform checks and append a dated row | At M8, before the public announcement | A conflict can appear at any time; the M0 result is a snapshot |
| Create the itch.io page `eonmark.itch.io` | M8 | Claims the slug; releases are published there and on GitHub |

A trademark hit in class 9 or 41 for games or software is a stop: switch to a runner-up before announcing. A hit in an unrelated class (clothing, cosmetics) is noted and does not block.

## Runner-up names

In order of preference. Each was checked on crates.io, Steam and GitHub on 2026-10-05 unless marked otherwise.

| Name | Rationale | Status |
|---|---|---|
| Marchfall | "March" is a frontier province; slightly martial; second choice if a trademark conflict ever appears | crates.io 404, Steam 0, GitHub 0 |
| Hearthmarch | Hearth (towns as anchors) plus march (border land); warm and calm; longer to type | crates.io 404, Steam 0, GitHub 0 |
| Boundstone | Boundary stones mark borders; a GitHub org `boundstone` and an unrelated repository already exist, so the slug is contested | crates.io 404, Steam 0, itch 0 |
| Cadastre | The land register of parcel ownership; thematically exact but a generic GIS term with many unrelated repositories and an existing Rust strategy game using the word | crates.io 404, Steam 0 |
| Bournmark | "Bourn" (archaic boundary) plus "mark"; obscure enough that conflicts are unlikely | Not individually verified; fallback only |
| Isoline | The contour where ownership changes, which is literally the border shader; likely collides with cartography software | Not verified; lowest priority |

Rejected outright: Demesne (existing Steam game, 2016), Marchland (existing board game), Tilth and Ambit (crates.io names taken).

## No placeholder crates

crates.io forbids name squatting, so no empty `eonmark`, `eonmark-sim` or `eonmark-rules` crate is published to reserve the name. The workspace crates are `publish = false`. If the owner wants crates published, `eonmark-sim` and `eonmark-rules` ship with real content after M1 and M3a respectively; this is a v0.2 roadmap item in [ROADMAP.md](ROADMAP.md). Name protection comes from the repository, the domain and the itch.io page, not from copyleft and not from placeholder crates.

## Policy for mentioning the inspiration

Eonmark uses original names for everything: ages, resources, the faction, units, buildings, identifiers, data, assets, UI strings and commit messages. The inspiration's title appears only in [README.md](../README.md) (the single disclaimed sentence) and [design/prior-art.md](design/prior-art.md) (its Inspiration section and source citations). Its common abbreviation appears nowhere. `scripts/check_trademark.sh` enforces this in CI. See [adr/0001-license.md](adr/0001-license.md) for the reasoning.
