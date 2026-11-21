#!/usr/bin/env bash
# The caller has tested these five native executables on their respective runners.
set -euo pipefail
tag="cli-v${CLI_VERSION}"
mkdir -p release
for platform in darwin-arm64 darwin-x64 linux-arm64 linux-x64 win32-x64; do
  executable=agent-context
  name="agent-context-$platform"
  if [[ "$platform" == win32-* ]]; then executable+=.exe; name+=.exe; fi
  install -m 755 "artifacts/$platform/$executable" "release/$name"
done
existing=$(git rev-parse --verify --quiet "refs/tags/$tag^{commit}" || true)
if [[ -n "$existing" && "$existing" != "$GITHUB_SHA" ]]; then
  echo "::error::Tag $tag already belongs to $existing"; exit 1
fi
state=$(gh api --paginate --slurp "repos/$GITHUB_REPOSITORY/releases?per_page=100" |
  jq -c --arg tag "$tag" '.[][] | select(.tag_name == $tag)')
if [[ -n "$state" && $(jq -r .draft <<< "$state") == false ]]; then
  for file in release/*; do
    digest="sha256:$(sha256sum "$file" | cut -d' ' -f1)"
    jq -e --arg name "${file##*/}" --arg digest "$digest" 'any(.assets[]; .name == $name and .digest == $digest)' <<< "$state" >/dev/null
  done
  exit 0
fi
if [[ -z "$state" ]]; then
  flags=(--draft --target "$GITHUB_SHA" --title "agent-context CLI $CLI_VERSION" --notes "Prebuilt command-line executables for $CLI_VERSION.")
  if [[ "${CLI_VERSION%%+*}" == *-* ]]; then flags+=(--prerelease); fi
  gh release create "$tag" "${flags[@]}"
elif [[ $(jq -r .target_commitish <<< "$state") != "$GITHUB_SHA" ]]; then
  echo "::error::Existing draft belongs to another commit"; exit 1
fi
gh release upload "$tag" release/* --clobber
gh release edit "$tag" --draft=false
