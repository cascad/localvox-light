#!/usr/bin/env bash
# Модели LocalVox → ~/.localvox/models (или настроенный/portable путь).
#
# Список качаемого лежит в scripts/models.json — это ДАННЫЕ. Здесь только механика:
# докачать недостающее, сверить sha256, распаковать. Повторный запуск ничего не
# переделывает: скрипт можно гонять сколько угодно раз, в том числе после обрыва связи.
#
#   ./scripts/fetch-models.sh                      # всё
#   ./scripts/fetch-models.sh --only gigaam        # только распознавание архива
#   ./scripts/fetch-models.sh --skip ner           # без проверки имён (экономит 580 МБ)
#   ./scripts/fetch-models.sh --required-only      # минимум, на котором продукт работает
#   ./scripts/fetch-models.sh --root /opt/localvox # не в репозиторий, а в установку
#   ./scripts/fetch-models.sh --check              # ничего не качать, только сказать чего нет
#
# Нативная библиотека Vosk ставится отдельно (setup-vosk.sh): она своя под каждую ОС.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
MANIFEST="$HERE/models.json"
ROOT=''
COMPONENT_DIR=''
ONLY=()
SKIP=()
REQUIRED_ONLY=0
CHECK_ONLY=0
REPAIR_ZIP=0

while [ $# -gt 0 ]; do
    case "$1" in
        --only) ONLY+=("$2"); shift 2 ;;
        --only=*) ONLY+=("${1#*=}"); shift ;;
        --skip) SKIP+=("$2"); shift 2 ;;
        --skip=*) SKIP+=("${1#*=}"); shift ;;
        --required-only) REQUIRED_ONLY=1; shift ;;
        --check) CHECK_ONLY=1; shift ;;
        --repair-zip) REPAIR_ZIP=1; shift ;;
        --root) ROOT="$2"; shift 2 ;;
        --root=*) ROOT="${1#*=}"; shift ;;
        --component-dir) COMPONENT_DIR="$2"; shift 2 ;;
        -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
        *) echo "Неизвестный аргумент: $1" >&2; exit 2 ;;
    esac
done

command -v curl >/dev/null || { echo "Нужен curl" >&2; exit 1; }
command -v jq   >/dev/null || { echo "Нужен jq (читать models.json). Поставить: brew install jq / apt install jq" >&2; exit 1; }
command -v unzip >/dev/null || { echo "Нужен unzip" >&2; exit 1; }
[ -f "$MANIFEST" ] || { echo "Не найден $MANIFEST" >&2; exit 1; }

if [[ -n "$COMPONENT_DIR" ]]; then
 [[ ${#ONLY[@]} = 1 ]] || { echo '--component-dir requires exactly one --only' >&2; exit 2; }
 jq -e --arg id "${ONLY[0]}" 'any(.components[]; .id == $id)' "$MANIFEST" >/dev/null || exit 2
 [[ "$COMPONENT_DIR" = /* ]] || COMPONENT_DIR="$PWD/$COMPONENT_DIR"
 ROOT="$COMPONENT_DIR"
fi
source "$HERE/model-paths.sh"
model_locations "$(cd "$HERE/.." && pwd)" "$ROOT"
if [[ -n "$COMPONENT_DIR" ]]; then MODEL_LOCATIONS="$(jq --arg id "${ONLY[0]}" --arg path "$COMPONENT_DIR" '.components |= map(if .id == $id then .path=$path | .source="root" else . end)' <<<"$MODEL_LOCATIONS")"; fi

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
    else shasum -a 256 "$1" | cut -d' ' -f1; fi          # macOS
}

wanted() {
    local id="$1" required="$2" w s
    for s in ${SKIP[@]+"${SKIP[@]}"}; do [ "$s" = "$id" ] && return 1; done
    if [ ${#ONLY[@]} -gt 0 ]; then
        for w in "${ONLY[@]}"; do [ "$w" = "$id" ] && return 0; done
        return 1
    fi
    [ "$REQUIRED_ONLY" = 1 ] && [ "$required" != "true" ] && return 1
    return 0
}

MISSING=0

# Скачивание идёт в «.part» и переименовывается только после проверки. Оборванная закачка
# на 1.8 ГБ иначе осталась бы лежать под правильным именем и при следующем запуске
# сошла бы за готовый файл — а это молча испорченная модель.
fetch_verified() {
    local url="$1" target="$2" want_sha="$3" note="$4"
    new_model_parent "$target"
    local part="$target.part"
    echo "  ⇣ $(basename "$target")"
    curl -fL --progress-bar -A "localvox-fetch-models/1.0" "$url" > "$part"

    if [ "$want_sha" != "null" ] && [ -n "$want_sha" ]; then
        local got; got="$(sha256_of "$part")"
        if [ "$got" != "$want_sha" ]; then
            rm -f "$part"
            echo "    sha256 НЕ СОВПАЛ: ждали $want_sha, получили $got. Файл удалён." >&2
            exit 1
        fi
        echo "    sha256 совпал"
    elif [ -n "$note" ] && [ "$note" != "null" ]; then
        echo "    sha256 вверху не публикуют — $note"
    fi
    mv -f "$part" "$target"
}

n_comp="$(jq '.components | length' "$MANIFEST")"
for ci in $(seq 0 $((n_comp - 1))); do
    comp="$(jq -c ".components[$ci]" "$MANIFEST")"
    id="$(jq -r '.id' <<<"$comp")"
    title="$(jq -r '.title' <<<"$comp")"
    approx="$(jq -r '.approx' <<<"$comp")"
    required="$(jq -r '.required' <<<"$comp")"

    wanted "$id" "$required" || { echo "— $title: пропущено"; continue; }
    echo "$title (~$approx)"
    component_dir="$(jq -r '.directory' <<<"$comp")"
    component_path="$(jq -r --arg id "$id" '.components[] | select(.id == $id) | .path // ""' <<<"$MODEL_LOCATIONS")"
    location_source="$(jq -r --arg id "$id" '.components[] | select(.id == $id) | .source' <<<"$MODEL_LOCATIONS")"
    [[ -n "$component_path" ]] || { echo "No model directory for $id. Set its model env or use --root." >&2; exit 1; }
    if command -v cygpath >/dev/null 2>&1; then component_path="$(cygpath -u "$component_path")"; fi
    echo "  $component_path [$location_source]"

    n_items="$(jq '.items | length' <<<"$comp")"
    for ii in $(seq 0 $((n_items - 1))); do
        item="$(jq -c ".items[$ii]" <<<"$comp")"
        kind="$(jq -r '.kind' <<<"$item")"

        case "$kind" in
        file)
            dest="$(model_payload "$component_path" "$component_dir" "$(jq -r '.dest' <<<"$item")")"
            if [ -f "$dest" ]; then echo "  уже на месте: $(basename "$dest")"; continue; fi
            if [ "$CHECK_ONLY" = 1 ]; then echo "  НЕТ: $(jq -r '.dest' <<<"$item")"; MISSING=1; continue; fi
            [[ "$location_source" != explicit ]] || { echo "Configured model is incomplete: $component_path. Choose download/repair in --setup or use an explicit --root." >&2; exit 1; }
            fetch_verified "$(jq -r '.url' <<<"$item")" "$dest" \
                "$(jq -r '.sha256' <<<"$item")" "$(jq -r '.sha256_note // ""' <<<"$item")"
            ;;
        text)
            dest="$(model_payload "$component_path" "$component_dir" "$(jq -r '.dest' <<<"$item")")"
            if [[ "$CHECK_ONLY" = 1 || "$location_source" = explicit ]]; then continue; fi
            [ -f "$dest" ] && continue
            new_model_parent "$dest"
            jq -r '.content[]' <<<"$item" > "$dest"
            ;;
        zip)
            unpack_to="$(model_payload "$component_path" "$component_dir" "$(jq -r '.unpack_to' <<<"$item")")"
            marker="$(jq -r '.marker' <<<"$item")"
            if [ "$REPAIR_ZIP" = 0 ] && [ -f "$unpack_to/$marker" ]; then echo "  уже на месте: $(jq -r '.unpack_to' <<<"$item")"; continue; fi
            if [ "$CHECK_ONLY" = 1 ]; then echo "  НЕТ: $(jq -r '.unpack_to' <<<"$item")"; MISSING=1; continue; fi
            [[ "$location_source" != explicit ]] || { echo "Configured model needs repair: $component_path. Choose download/repair in --setup or use an explicit --root." >&2; exit 1; }
            unpack_to="$(assert_model_replacement "$unpack_to" "$component_path")"

            tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
            fetch_verified "$(jq -r '.url' <<<"$item")" "$tmp/a.zip" \
                "$(jq -r '.sha256' <<<"$item")" "$(jq -r '.sha256_note // ""' <<<"$item")"
            echo "  распаковка…"
            unzip -q "$tmp/a.zip" -d "$tmp/x"
            # Архив несёт свой корневой каталог (vosk-model-ru-0.42/…). Кладём его СОДЕРЖИМОЕ
            # туда, куда просит манифест, а не полагаемся на совпадение имён: перезальют архив
            # с другим корнем — и без этого мы молча получим models/models/….
            if [ "$(jq -r '.strip_root' <<<"$item")" = "true" ]; then
                inner="$(find "$tmp/x" -mindepth 1 -maxdepth 1 -type d | head -1)"
                [ -n "$inner" ] || { echo "В архиве нет корневого каталога" >&2; exit 1; }
            else
                inner="$tmp/x"
            fi
            [ -f "$inner/$marker" ] || { echo "Downloaded archive has no $marker" >&2; exit 1; }
            unpack_to="$(assert_model_replacement "$unpack_to" "$component_path")"
            rm -rf "$unpack_to"
            new_model_parent "$unpack_to"
            mv "$inner" "$unpack_to"
            [ -f "$unpack_to/$marker" ] || { echo "После распаковки нет $marker в $unpack_to" >&2; exit 1; }
            rm -rf "$tmp"; trap - EXIT
            ;;
        *) echo "Неизвестный kind: $kind" >&2; exit 2 ;;
        esac
    done
done

echo
if [ "$CHECK_ONLY" = 1 ]; then
    [ "$MISSING" = 0 ] && echo "всё на месте" || { echo "чего-то не хватает — запусти без --check"; exit 1; }
else
    echo "готово"
fi
