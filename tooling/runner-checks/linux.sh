#!/usr/bin/env bash
set -uo pipefail

system=linux
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=tooling/runner-checks/report.sh
source "$script_dir/report.sh" || exit 1
deadline=$((SECONDS + ${YARD_CHECK_SECONDS:-10200}))

checks="$PWD/checks/linux"
mkdir -p "$checks" || exit 1
report="$checks/report.txt"
: > "$report" || exit 1
unsigned_cases=(bundle bundle-secrets desktop-file-validate install url-cold url-warm screenshot)
expected_cases=('plain coexistence')
for config in plain spaced other; do
    for case in "${unsigned_cases[@]}"; do expected_cases+=("$config $case"); done
done

work=$(mktemp -d) || exit 1
export YARD_HOME="$work/yard-home"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
unset MACOS_CERTIFICATE MACOS_CERTIFICATE_PASSWORD WINDOWS_CERTIFICATE WINDOWS_CERTIFICATE_PASSWORD
unset APPLE_NOTARIZATION_KEY APPLE_NOTARIZATION_KEY_ID APPLE_NOTARIZATION_ISSUER_ID

bounded() {
    local limit=$1 remaining=$((deadline - SECONDS))
    shift
    if (( remaining <= 0 )); then return 124; fi
    if (( limit > remaining )); then limit=$remaining; fi
    timeout -k 5s "${limit}s" "$@"
}

stop_app() {
    if [[ -n ${app_binary:-} ]]; then
        pkill -TERM -f -x "$app_pattern" 2>/dev/null || true
        sleep 1
        pkill -KILL -f -x "$app_pattern" 2>/dev/null || true
    fi
}

cleanup() {
    if [[ -f "$work/coexist-pids" ]]; then
        while IFS= read -r pid; do kill "$pid" 2>/dev/null || true; done < "$work/coexist-pids"
    fi
    stop_app
    if [[ -n ${opener:-} ]]; then wait "$opener" || true; fi
    rm -rf "$work"
}
trap 'if (( $? != 0 )); then failed=1; fi; cleanup; finish_report; exit $?' EXIT
trap 'failed=1; exit 1' INT TERM

check_bundle() {
    local status=0
    rm -f "$CARGO_TARGET_DIR"/{xtask.log,xtask.log.old,lifecycle.jsonl}
    bounded 1500 cargo xtask bundle linux --package yard --config "$fixture" --out "$dist" \
        > "$logs/bundle.log" 2>&1 || status=$?
    for file in xtask.log xtask.log.old lifecycle.jsonl; do
        if [[ -f "$CARGO_TARGET_DIR/$file" ]]; then cp "$CARGO_TARGET_DIR/$file" "$logs/xtask-$file"; fi
    done
    # The unsigned invocation has no signing secrets to search for.
    record PASS bundle-secrets 'no signing credentials supplied'
    if (( status == 0 )) && [[ -f "$archive" ]]; then
        record PASS bundle "created $(basename "$archive")"
        return 0
    fi
    record FAIL bundle "exit $status or archive missing (see $config/bundle.log)"
    return 1
}

check_desktop() {
    if desktop-file-validate "$dist/$slug.app/share/applications/$identifier.desktop" \
        > "$logs/desktop.log" 2>&1; then
        record PASS desktop-file-validate "$identifier.desktop accepted"
    else
        record FAIL desktop-file-validate "see $config/desktop.log"
    fi
}

check_install() {
    mkdir -p "$dist/unpack"
    if bounded 60 tar -xzf "$archive" -C "$dist/unpack" > "$logs/install.log" 2>&1 &&
        bounded 60 sh "$dist/unpack/$slug.app/install.sh" >> "$logs/install.log" 2>&1 &&
        [[ -x "$app_binary" ]]; then
        record PASS install "$app_binary installed"
        return 0
    fi
    record FAIL install "see $config/install.log"
    return 1
}

check_cold_url() {
    local counts urls requests forwarded
    if ! counts=$(trace_counts "$trace" "$scheme"); then
        record FAIL url-cold 'could not read the URL trace before launch'
        return 1
    fi
    read -r urls requests forwarded <<< "$counts"
    bounded 125 xdg-open "$scheme://nobody/cold-$RANDOM-$RANDOM" > "$logs/xdg-cold.log" 2>&1 &
    opener=$!
    if wait_for_counts "$trace" "$scheme" "$((urls + 1))" "$requests" "$forwarded"; then
        cold_pid=$(pgrep -f -x "$app_pattern")
        if [[ $cold_pid =~ ^[0-9]+$ ]]; then return 0; fi
    fi
    local status=0
    stop_app
    wait "$opener" || status=$?
    opener=
    record FAIL url-cold "no single process with exactly one URL dispatch; xdg-open exit $status"
    return 1
}

send_warm_url() {
    local case=$1 counts urls requests forwarded pid
    pid=$(pgrep -f -x "$app_pattern") || return 1
    [[ $pid =~ ^[0-9]+$ ]] || return 1
    counts=$(trace_counts "$trace" "$scheme") || return 1
    read -r urls requests forwarded <<< "$counts"
    bounded 15 xdg-open "$scheme://nobody/$case-$RANDOM-$RANDOM" > "$logs/$case.log" 2>&1 &&
        wait_for_counts "$trace" "$scheme" "$((urls + 1))" "$((requests + 1))" "$((forwarded + 1))" &&
        [[ $(pgrep -f -x "$app_pattern") == "$pid" ]]
}

finish_cold_url() {
    local status=0 window
    # Some xdg-open backends wait for the launched app to exit.
    if window=$(bounded 5 xdotool search --onlyvisible --pid "$cold_pid" | head -n 1) &&
        [[ -n "$window" ]]; then
        bounded 5 xdotool windowactivate --sync "$window" key alt+F4 || true
    fi
    wait "$opener" || status=$?
    opener=
    if (( status == 0 )); then
        record PASS url-cold "reached the app's URL handler at PID $cold_pid"
    else
        record FAIL url-cold "xdg-open exited $status (see xdg-cold.log)"
    fi
}

check_screenshot() {
    if bounded 1500 env PRODUCT_CONFIG="$fixture" YARD_LIFECYCLE=info,screenshot=debug \
        cargo run -p screenshot -- --out "$logs/window.png" > "$logs/screenshot.log" 2>&1 &&
        [[ -s "$logs/window.png" ]]; then
        record PASS screenshot 'GPUI headless window captured'
    else
        record FAIL screenshot "see $config/screenshot.log"
    fi
}

load_config() {
    config=$1
    fixture="$PWD/crates/yard_build_config/tests/configs/$config/product.toml"
    read_product || return 1
    slug=$(printf '%s' "$name" | LC_ALL=C tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-|-$//g')
    prefix=$(printf '%s' "$name" | LC_ALL=C tr '[:lower:]' '[:upper:]' | sed 's/[^A-Z0-9]/_/g')
    export XDG_DATA_HOME="$work/data" XDG_CONFIG_HOME="$work/config"
    logs="$checks/$config"
    mkdir -p "$logs" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"
    dist="$work/$config/dist"
    archive="$dist/$slug-$version-linux-$(uname -m).tar.gz"
    app_binary="$HOME/.local/$slug.app/bin/$slug"
    escaped_binary=$(printf '%s' "$app_binary" | sed 's/[][\\.^$*+?(){}|]/\\&/g')
    app_pattern="$escaped_binary( .*)?"
    trace="$XDG_DATA_HOME/$slug/logs/lifecycle.jsonl"
}

default_handler() {
    [[ $(bounded 10 xdg-mime query default "x-scheme-handler/$scheme") == "$identifier.desktop" ]]
}

for fixture_name in plain spaced other; do
    if (( SECONDS >= deadline )); then break; fi
    if ! load_config "$fixture_name"; then
        skip 'missing or blank required product field' "${unsigned_cases[@]}"
        continue
    fi
    export "${prefix}_LIFECYCLE=info,yard_shell=debug"
    opener=
    if ! check_bundle; then
        skip 'bundle failed' desktop-file-validate install url-cold url-warm
    else
        check_desktop
        if ! check_install; then
            skip 'install failed' url-cold url-warm
        elif ! default_handler; then
            skip 'the installed desktop file is not the default scheme handler' url-cold url-warm
        elif pgrep -f -x "$app_pattern" >/dev/null; then
            skip 'the app was already running' url-cold url-warm
        elif check_cold_url; then
            if send_warm_url warm && [[ $(pgrep -f -x "$app_pattern") == "$cold_pid" ]]; then
                record PASS url-warm "reached the app's URL handler at the same PID $cold_pid"
            else
                record FAIL url-warm 'no single forwarded request and URL dispatch in the original process'
            fi
            finish_cold_url
        else
            skip 'cold launch failed' url-warm
        fi
    fi
    stop_app
    if [[ -n "$opener" ]]; then wait "$opener" || true; opener=; fi
    if [[ -d "${trace%/*}" ]]; then cp -R "${trace%/*}" "$logs/app-logs"; fi
    app_binary=
    check_screenshot
    unset "${prefix}_LIFECYCLE"
done

check_coexistence() {
    local product plain_trace plain_pattern plain_pid other_trace other_pid
    local plain_before other_before attempt
    for product in plain other; do
        load_config "$product" && [[ -x "$app_binary" ]] && default_handler || return 1
        # Preserve the instance paths used by a normal installed launch.
        rm -f "$trace" || return 1
        env "${prefix}_LIFECYCLE=info,yard_shell=debug" "$app_binary" > "$logs/coexist-app.log" 2>&1 &
        printf '%s\n' "$!" >> "$work/coexist-pids" || return 1
        for ((attempt=0; attempt<30 && SECONDS<deadline; attempt++)); do
            if grep -Fq '"name":"window-open"' "$trace" 2>/dev/null; then break; fi
            sleep 1
        done
        grep -Fq '"name":"window-open"' "$trace" || return 1
    done
    other_trace=$trace
    other_pid=$(pgrep -f -x "$app_pattern") || return 1
    load_config plain || return 1
    plain_trace=$trace plain_pattern=$app_pattern
    plain_pid=$(pgrep -f -x "$plain_pattern") || return 1
    [[ $plain_pid =~ ^[0-9]+$ && $other_pid =~ ^[0-9]+$ && $plain_pid != "$other_pid" ]] || return 1

    other_before=$(trace_counts "$other_trace" "*") || return 1
    send_warm_url coexist-plain &&
        [[ $(trace_counts "$other_trace" "*") == "$other_before" ]] || return 1
    plain_before=$(trace_counts "$plain_trace" "*") || return 1
    load_config other && send_warm_url coexist-other &&
        [[ $(trace_counts "$plain_trace" "*") == "$plain_before" ]] &&
        [[ $(pgrep -f -x "$plain_pattern") == "$plain_pid" && $(pgrep -f -x "$app_pattern") == "$other_pid" ]] &&
        cp "$plain_trace" "$checks/plain/coexist-trace.jsonl" &&
        cp "$other_trace" "$checks/other/coexist-trace.jsonl"
}

if (( SECONDS < deadline )); then
    if check_coexistence; then
        config=plain
        record PASS coexistence "two PIDs; each URL reached its own app's URL handler only"
    else
        config=plain
        record FAIL coexistence 'expected two running apps and isolated URL delivery'
    fi
fi
