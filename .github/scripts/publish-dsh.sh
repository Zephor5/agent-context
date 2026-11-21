#!/usr/bin/env bash
set -euo pipefail
manifest=integrations/dsh/package.json
name=$(jq -er .name "$manifest")
version=$(jq -er .version "$manifest")
archive="./release/zephor-dsh-agent-context-$version.tgz"
[[ "$name" == '@zephor/dsh-agent-context' ]]
tar -xOzf "$archive" package/package.json | jq -e --arg n "$name" --arg v "$version" '.name == $n and .version == $v'
registry=https://registry.npmjs.org
status=$(curl -sS --retry 3 -w '%{http_code}' -o "$RUNNER_TEMP/npm-version.json" "$registry/$name/$version")
case "$status" in
  200)
    digest="sha512-$(openssl dgst -sha512 -binary "$archive" | openssl base64 -A)"
    jq -e --arg digest "$digest" '.dist.integrity == $digest' "$RUNNER_TEMP/npm-version.json"
    echo "$name@$version is already published with identical contents"
    exit 0 ;;
  404) ;;
  *) echo "::error::npm version lookup failed: HTTP $status"; exit 1 ;;
esac
args=()
if [[ "${version%%+*}" == *-* ]]; then args+=(--tag next); fi
npm publish "$archive" --access public --registry "$registry" --provenance --ignore-scripts "${args[@]}"
