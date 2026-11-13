# Shared placement for fetch-models; never source/eval a user's .env.
model_locations() {
    local install="$1" root="$2" exe="$1/localvox-light" report home_dir="${HOME:-}" id env_key directory parent candidate value source
    USER_MODEL_ROOT=''
    if [[ ! ${HOME+x} ]]; then home_dir=~; fi
    jq -e 'all(.components[]; (.directory | test("^models/[^/\\\\:]+$")) and (.directory != "models/..") and (.directory != "models/."))' "$MANIFEST" >/dev/null || { echo 'Unsafe component directory' >&2; return 1; }
    [[ "$home_dir" = /* ]] && USER_MODEL_ROOT="$home_dir/$(jq -r '.storage.user_directory' "$MANIFEST")"
    # MSYS can mount the same temporary directory as both /d/... and /tmp.
    if [[ -n "$USER_MODEL_ROOT" ]] && command -v cygpath >/dev/null 2>&1; then USER_MODEL_ROOT="$(cygpath -u "$(cygpath -am "$USER_MODEL_ROOT")")"; fi
    [[ -f "$exe.exe" ]] && exe="$exe.exe"
    if [[ -z "$root" && -f "$exe" ]]; then
        report="$(cd "$install" && "$exe" --model-paths-json)" || { echo 'Model path resolver failed' >&2; return 1; }
        jq -e '.schema == 1 and (.components | type == "array")' <<<"$report" >/dev/null || { echo 'Update the bundle: unsupported model path resolver schema' >&2; return 1; }
        MODEL_LOCATIONS="$report"
        USER_MODEL_ROOT="$(jq -r '.user_root // ""' <<<"$report")"
        if [[ -n "$USER_MODEL_ROOT" ]] && command -v cygpath >/dev/null 2>&1; then USER_MODEL_ROOT="$(cygpath -u "$USER_MODEL_ROOT")"; fi
        return
    fi
    if [[ -z "$root" && ( -f "$install/.env" || -f .env ) ]]; then
        echo 'Reading .env requires the installed localvox-light resolver. Use the bundle or an explicit --root.' >&2; return 1
    fi
    MODEL_LOCATIONS='{"components":[]}'
    while IFS=$'\t' read -r id env_key directory parent; do
        value=''; source=unavailable
        if [[ -n "$root" ]]; then
            [[ "$root" = /* ]] || root="$PWD/$root"
            value="$root/$directory"; source=root
        elif printenv "$env_key" >/dev/null; then
            value="${!env_key}"; source=explicit
            # Same whitespace/paired-quote normalization as the native resolver.
            value="${value#"${value%%[![:space:]]*}"}"; value="${value%"${value##*[![:space:]]}"}"
            if [[ "$value" = \"*\" || "$value" = \'*\' ]]; then value="${value:1:${#value}-2}"; fi
            value="${value#"${value%%[![:space:]]*}"}"; value="${value%"${value##*[![:space:]]}"}"
            if [[ -n "$value" ]] && command -v cygpath >/dev/null 2>&1; then value="$(cygpath -u "$value")"; fi
            [[ -z "$value" || "$value" = /* ]] || value="$PWD/$value"
        else
            if [[ -n "$parent" ]]; then
                candidate="$(jq -r --arg id "$parent" '.components[] | select(.id == $id) | .path // ""' <<<"$MODEL_LOCATIONS")"
                if [[ -n "$candidate" && -d "$(dirname "$candidate")/$(basename "$directory")" ]]; then
                    value="$(dirname "$candidate")/$(basename "$directory")"; source=asr
                fi
            fi
            if [[ -z "$value" ]]; then
                for candidate in "$install/$directory" "$PWD/$directory"; do
                    if [[ -d "$candidate" ]]; then
                        value="$candidate"; source=cwd
                        [[ "$candidate" != "$install/$directory" ]] || source=portable
                        break
                    fi
                done
            fi
            if [[ -z "$value" && -n "$USER_MODEL_ROOT" ]]; then value="$USER_MODEL_ROOT/$directory"; source=user; fi
        fi
        MODEL_LOCATIONS="$(jq --arg id "$id" --arg path "$value" --arg source "$source" '.components += [{id:$id,path:$path,source:$source}]' <<<"$MODEL_LOCATIONS")"
    done < <(jq -r '.components[] | [.id,.env,.directory,(.sibling_of // "")] | @tsv' "$MANIFEST")
}

new_model_parent() {
    local target user_root=''
    target="$(physical_model_path "$1")" || return 1
    if [[ -n "$USER_MODEL_ROOT" ]]; then user_root="$(physical_model_path "$USER_MODEL_ROOT")" || return 1; fi
    if [[ -n "$user_root" && ! -e "$user_root" && "$target" = "$user_root/"* ]]; then
        mkdir -p "$user_root"
        if [[ "${OS:-}" = Windows_NT ]]; then attrib +h "$user_root"; fi
    fi
    mkdir -p "$(dirname "$target")"
}

model_payload() {
    local base="$1" directory="$2" payload="$3" relative
    if [[ "$payload" = "$directory" ]]; then printf '%s\n' "$base"; return; fi
    [[ "$payload" = "$directory/"* ]] || { echo "Payload outside component: $payload" >&2; return 1; }
    relative="${payload#"$directory/"}"
    case "/$relative/" in */../*|*\\*|*:*|*//* ) echo "Unsafe payload: $payload" >&2; return 1;; esac
    printf '%s/%s\n' "$base" "$relative"
}

physical_model_path() {
    local directory="$1" suffix=''
    [[ "$directory" = /* ]] || return 1
    # Resolve existing ancestors (e.g. macOS /var -> /private/var), retaining the
    # not-yet-created model suffix. No directories are created by this function.
    while [[ ! -d "$directory" ]]; do
        [[ "$directory" != / && ! -L "$directory" ]] || return 1
        suffix="/$(basename "$directory")$suffix"
        directory="$(dirname "$directory")"
    done
    directory="$(cd -P "$directory" && pwd -P)" || return 1
    # Junction resolution may switch between equivalent MSYS /d/... and /tmp mounts.
    if command -v cygpath >/dev/null 2>&1; then directory="$(cygpath -u "$(cygpath -am "$directory")")"; fi
    printf '%s%s\n' "${directory%/}" "$suffix"
}

assert_model_replacement() {
    local target="$1" expected="$2" physical_target physical_expected
    [[ "$target" = /* && "$target" != / && ! -L "$target" && ! -L "$expected" ]] || { echo "Unsafe model replacement or model symlink: $target" >&2; return 1; }
    physical_target="$(physical_model_path "$target")" || return 1
    physical_expected="$(physical_model_path "$expected")" || return 1
    [[ -n "$physical_target" && "$physical_target" = "$physical_expected" ]] || { echo "Model destination changed: $target" >&2; return 1; }
    printf '%s\n' "$physical_target"
}
