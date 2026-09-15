#!/usr/bin/env bash
# Run with: bash tests/launcher.sh (no Docker needed).
set -euo pipefail
project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
test_dir="$(mktemp -d)"
test_dir="$(cd -- "$test_dir" && pwd -P)"
trap 'rm -rf -- "$test_dir"' EXIT
mkdir -p "$test_dir/bin" "$test_dir/project" "$test_dir/book pages" "$test_dir/model"
cp "$project_dir/afterprint" "$test_dir/project/afterprint"
cat >"$test_dir/bin/docker" <<'MOCK'
#!/usr/bin/env bash
set -eu
case "$*" in
    "image inspect afterprint:local") exit "${IMAGE_MISSING:-0}" ;;
    *" build "*) echo build >>"$LAUNCHER_LOG.build"; exit "${BUILD_EXIT:-0}" ;;
    *" run "*)
        printf '%s\n' "$AFTERPRINT_SCANS" "$AFTERPRINT_OUT" "$AFTERPRINT_MODEL" "$@" >"$LAUNCHER_LOG"
        exit "${LAUNCHER_EXIT:-0}" ;;
esac
MOCK
chmod +x "$test_dir/bin/docker"
export PATH="$test_dir/bin:$PATH" LAUNCHER_LOG="$test_dir/call"
cd "$test_dir"
launcher="$test_dir/project/afterprint"
bash "$launcher" --help >/dev/null
[[ ! -e $LAUNCHER_LOG ]]
bash "$launcher" 'book pages' --out 'result pages' --translation-model-path model --ocr-language spa
expected_root="$(pwd -P)"
grep -Fx -- "$expected_root/book pages" "$LAUNCHER_LOG"
grep -Fx -- "$expected_root/result pages" "$LAUNCHER_LOG"
grep -Fx -- "$expected_root/model" "$LAUNCHER_LOG"
grep -Fx -- '/model' "$LAUNCHER_LOG"
grep -Fx -- 'spa' "$LAUNCHER_LOG"
[[ -d 'result pages' && -d project/.afterprint ]]
bash "$launcher" 'book pages'
grep -Fx -- "$expected_root/out" "$LAUNCHER_LOG"
[[ ! -e $LAUNCHER_LOG.build ]]
grep -Fx -- '--pull' "$LAUNCHER_LOG"
grep -Fx -- 'never' "$LAUNCHER_LOG"
IMAGE_MISSING=1 bash "$launcher" 'book pages'
[[ $(wc -l <"$LAUNCHER_LOG.build") -eq 1 ]]
bash "$launcher" 'book pages' --build --translation-source ro --translation-target en
[[ $(wc -l <"$LAUNCHER_LOG.build") -eq 2 ]]
grep -Fx -- 'ro' "$LAUNCHER_LOG"
grep -Fx -- 'en' "$LAUNCHER_LOG"
if grep -Fxq -- '--build' "$LAUNCHER_LOG"; then exit 1; fi
if BUILD_EXIT=9 bash "$launcher" 'book pages' --build; then
    exit 1
else
    [[ $? == 9 ]]
fi
bash "$launcher" 'book pages' --out=equals --translation-model-path=model
grep -Fx -- "$expected_root/equals" "$LAUNCHER_LOG"
bash "$launcher" 'book pages' -o short
grep -Fx -- "$expected_root/short" "$LAUNCHER_LOG"
if bash "$launcher" missing 2>/dev/null; then exit 1; fi
if bash "$launcher" 'book pages' --out 2>/dev/null; then exit 1; fi
if bash "$launcher" 'book pages' --translation-model-path missing 2>/dev/null; then exit 1; fi
if LAUNCHER_EXIT=7 bash "$launcher" 'book pages'; then
    exit 1
else
    [[ $? == 7 ]]
fi
echo 'Launcher checks passed.'
