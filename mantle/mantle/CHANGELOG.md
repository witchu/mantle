# mantle

## 0.12.0

### Minor Changes

- add `--skip-place-files` flag to `deploy` to deploy everything except place files (d2ec75a)
- add `--reconcile-thumbnails` and `--prune-thumbnails` flags to `deploy` (8217121)
  - compares thumbnail IDs on Roblox with the state file before deploying
  - thumbnails missing from Roblox are uploaded again and the thumbnail order is set again
  - untracked thumbnails on Roblox are reported, and deleted with `--prune-thumbnails`
- add `--reconcile` flag to `deploy` (8450c5a)
  - reconciles thumbnails like `--reconcile-thumbnails`
  - adopts developer products, game passes and badges which exist on Roblox with the same name but are missing from the state file, instead of creating duplicates

### Patch Changes

- keep the state file consistent when re-uploading a thumbnail fails after the old one was deleted (e.g. due to rate limiting), which caused extra thumbnails and `One or more of the provided thumbnail ids are not valid` errors (22867ff)
- treat thumbnails which no longer exist on Roblox as deleted instead of failing every deployment (22867ff)
- keep the state file consistent when re-creating an experience fails after the old one was archived (8569a8a)
- only upload game pass icons when the icon file changes, and store the new icon ID in the state file (7690f44)
- fix listing game passes, developer products, places and badges never finishing when Roblox returns an empty next page token (39f707b)
- stop sending a price when deprecating a developer product (21c1bf8)
- add API documentation links to the README and fix lint warnings (6778ac1)

## 0.11.19

### Patch Changes

- migrate developer products to the new developer products v2 API (483b7c4)
  - product icons are now part of the product resource, and existing state files are migrated automatically (state file v7)
- migrate game passes to the new game passes API (4859fad)
- upgrade the Rust toolchain to 1.96.0 (f01b0ec)
- clarify Cloudflare R2 credentials in the authentication docs (79fcbfa)

## 0.11.5

### Patch Changes

- automatically grant new audio assets permission to the target experience
