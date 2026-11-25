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
northstar_cases=(new-app bundle sample-bundle install sample-install desktop-name desktop-exec desktop-icon
    desktop-mime url screenshot coexistence)
for case in "${northstar_cases[@]}"; do expected_cases+=("northstar $case"); done
update_cases=(update-clone update-new-app update update-app-kept update-manifest bundle install
    desktop-name desktop-exec desktop-icon desktop-mime url)
for case in "${update_cases[@]}"; do expected_cases+=("update $case"); done

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

# The PNG holds more than one colour: a blank capture is all black.
has_colours() {
    local count
    count=$(bounded 30 convert "$1" -format %k info: 2>> "${1%.png}-colours.log") &&
        [[ $count =~ ^[0-9]+$ ]] && (( count > 1 ))
}

check_screenshot() {
    # The screenshot runs the sample's composition under the fixture's product,
    # which reads its own lifecycle variable; it sets its own home.
    if bounded 1500 env PRODUCT_CONFIG="$fixture" "${prefix}_LIFECYCLE=info,screenshot=debug" \
        cargo run -p screenshot -- --out "$logs/window.png" > "$logs/screenshot.log" 2>&1 &&
        [[ -s "$logs/window.png" ]] && has_colours "$logs/window.png"; then
        record PASS screenshot 'GPUI headless window captured'
    else
        record FAIL screenshot "missing or single-colour window.png (see $config/screenshot.log)"
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

# A developer's application, made with `cargo xtask new-app` in a fresh
# checkout, installed beside the sample bundled there with `--package yard`.
# Each starts with its own home and lifecycle variables.
fresh="$work/fresh"
northstar_icon=crates/yard_build_config/tests/configs/other/icon.png
fresh_lifecycle='info,yard_shell=debug,yard_paths=debug'
# Seconds to wait before counts are read again, to catch a late delivery.
settle=3

trace_fact() {
    python3 "$script_dir/trace-facts.py" "$1" "$2"
}

# Loads the fresh checkout's product from app/$1/, with its files under $2.
load_fresh() {
    fixture="$fresh/app/$1/product.toml"
    read_product || return 1
    slug=$(printf '%s' "$name" | LC_ALL=C tr '[:upper:]' '[:lower:]' | sed -E 's/[^a-z0-9]+/-/g; s/^-|-$//g')
    prefix=$(printf '%s' "$name" | LC_ALL=C tr '[:lower:]' '[:upper:]' | sed 's/[^A-Z0-9]/_/g')
    export XDG_DATA_HOME="$work/data" XDG_CONFIG_HOME="$work/config"
    logs="$checks/$config/$2"
    home="$work/$config/$2/home"
    dist="$work/$config/$2/dist"
    mkdir -p "$logs" "$home" "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" || return 1
    archive="$dist/$slug-$version-linux-$(uname -m).tar.gz"
    install_dir="$HOME/.local/$slug.app"
    app_binary="$install_dir/bin/$slug"
    escaped_binary=$(printf '%s' "$app_binary" | sed 's/[][\\.^$*+?(){}|]/\\&/g')
    app_pattern="$escaped_binary( .*)?"
    trace="$home/logs/lifecycle.jsonl"
}

load_north_star() { load_fresh north-star north-star; }
load_sample() { load_fresh yard sample; }
load_upstream_star() { load_fresh upstream-star upstream-star; }

bundle_fresh() {
    local case=$1 label=$2 status=0 file
    shift 2
    rm -f "${CARGO_TARGET_DIR:?}"/{xtask.log,xtask.log.old,lifecycle.jsonl}
    (cd "$fresh" && bounded 1500 cargo xtask bundle linux "$@" --out "$dist") > "$logs/bundle.log" 2>&1 || status=$?
    for file in xtask.log xtask.log.old lifecycle.jsonl; do
        if [[ -f "$CARGO_TARGET_DIR/$file" ]]; then cp "$CARGO_TARGET_DIR/$file" "$logs/xtask-$file"; fi
    done
    if (( status == 0 )) && [[ -f "$archive" ]]; then
        record PASS "$case" "created $(basename "$archive")"
        return 0
    fi
    record FAIL "$case" "exit $status or archive missing (see $config/$label/bundle.log)"
    return 1
}

install_fresh() {
    local case=$1 label=$2
    mkdir -p "$dist/unpack"
    if bounded 60 tar -xzf "$archive" -C "$dist/unpack" > "$logs/install.log" 2>&1 &&
        bounded 60 sh "$dist/unpack/$slug.app/install.sh" >> "$logs/install.log" 2>&1 &&
        [[ -x "$app_binary" ]]; then
        record PASS "$case" "$app_binary installed"
        return 0
    fi
    record FAIL "$case" "see $config/$label/install.log"
    return 1
}

desktop_value() {
    sed -n "s/^$1=//p" "$XDG_DATA_HOME/applications/$identifier.desktop" 2>/dev/null
}

check_desktop_key() {
    local case=$1 key=$2 expected=$3 actual
    actual=$(desktop_value "$key")
    if [[ "$actual" == "$expected" ]]; then
        record PASS "$case" "$key=$actual"
    else
        record FAIL "$case" "$key expected $expected, got ${actual:-<missing>}"
    fi
}

check_desktop_icon() {
    local icon="$install_dir/share/icons/hicolor/512x512/apps/$identifier.png" actual
    actual=$(desktop_value Icon)
    if [[ "$actual" == "$icon" ]] && cmp -s "$icon" "$fresh/$northstar_icon" &&
        ! cmp -s "$icon" "$fresh/app/yard/icon.png"; then
        record PASS desktop-icon "Icon=$actual, the icon given to new-app"
    else
        record FAIL desktop-icon "Icon expected $icon holding the icon given to new-app, got ${actual:-<missing>}"
    fi
}

# Opens a URL of the loaded product with xdg-open, in that product's own
# environment, so that a second start finds the instance by the same home.
open_fresh_url() {
    local limit=$1 url=${3:-"$scheme://check/$2-$RANDOM-$RANDOM"}
    bounded "$limit" env "${prefix}_HOME=$home" "${prefix}_LIFECYCLE=$fresh_lifecycle" \
        xdg-open "$url" >> "$logs/url.log" 2>&1
}

# Starts the loaded product by opening its URL, and waits for one URL
# dispatch in a single new process. Sets fresh_pid.
start_fresh_url() {
    local status=0
    if pgrep -f -x "$app_pattern" >/dev/null; then return 1; fi
    rm -f "$trace" || return 1
    # Some xdg-open backends wait for the launched app to exit.
    open_fresh_url 125 cold "${1:-}" &
    printf '%s\n' "$!" >> "$work/coexist-pids" || return 1
    wait_for_counts "$trace" "$scheme" 1 0 0 || status=1
    fresh_pid=$(pgrep -f -x "$app_pattern") && [[ $fresh_pid =~ ^[0-9]+$ ]] || return 1
    printf '%s\n' "$fresh_pid" >> "$work/coexist-pids" || return 1
    return "$status"
}

# Sends a URL of the loaded product to its running instance, and waits for
# one more URL dispatch there, forwarded by the second start.
send_fresh_url() {
    local phase=$1 counts urls requests forwarded pid
    pid=$(pgrep -f -x "$app_pattern") && [[ $pid =~ ^[0-9]+$ ]] || return 1
    counts=$(trace_counts "$trace" "$scheme") || return 1
    read -r urls requests forwarded <<< "$counts"
    open_fresh_url 15 "$phase" &&
        wait_for_counts "$trace" "$scheme" "$((urls + 1))" "$((requests + 1))" "$((forwarded + 1))" &&
        [[ $(pgrep -f -x "$app_pattern") == "$pid" ]]
}

# The loaded product's trace names it, and holds exactly $1 URLs refused as
# `no such extension`.
in_own_trace() {
    [[ $(trace_fact "$trace" name) == "$name" &&
        $(trace_fact "$trace" identifier) == "$identifier" &&
        $(trace_fact "$trace" no-such-extension) == "$1" ]]
}

check_fresh_coexistence() {
    local north_home north_trace north_install north_pattern north_pid north_before
    local sample_home sample_trace sample_install sample_pattern sample_pid sample_before
    local north_value sample_value north_lock sample_lock
    load_north_star || return 1
    north_home=$home north_trace=$trace north_install=$install_dir north_pattern=$app_pattern
    why='the sample did not start from yard://check with one URL refused as no such extension, or North Star saw it'
    north_before=$(trace_counts "$north_trace" "*") &&
        load_sample && default_handler && start_fresh_url && sleep "$settle" && in_own_trace 1 &&
        [[ $(trace_counts "$north_trace" "*") == "$north_before" ]] || return 1
    sample_home=$home sample_trace=$trace sample_install=$install_dir sample_pattern=$app_pattern sample_pid=$fresh_pid
    why='expected one North Star and one sample process'
    north_pid=$(pgrep -f -x "$north_pattern") && [[ $north_pid =~ ^[0-9]+$ && $north_pid != "$sample_pid" ]] ||
        return 1

    # Each delivery is sampled again after a pause: the destination still has
    # exactly one more URL, and the peer none.
    why="northstar://check did not reach North Star alone (re-read after ${settle}s)"
    load_north_star && sample_before=$(trace_counts "$sample_trace" "*") && send_fresh_url coexist-north &&
        sleep "$settle" && in_own_trace 2 &&
        [[ $(trace_counts "$sample_trace" "*") == "$sample_before" ]] || return 1
    why="yard://check did not reach the sample alone (re-read after ${settle}s)"
    load_sample && north_before=$(trace_counts "$north_trace" "*") && send_fresh_url coexist-sample &&
        sleep "$settle" && in_own_trace 2 &&
        [[ $(trace_counts "$north_trace" "*") == "$north_before" ]] || return 1
    why='a process was replaced while URLs were delivered'
    [[ $(pgrep -f -x "$north_pattern") == "$north_pid" && $(pgrep -f -x "$sample_pattern") == "$sample_pid" ]] ||
        return 1

    why="install folders meet: $north_install and $sample_install"
    [[ $north_install != "$sample_install" && $north_install != "$sample_install"/* &&
        $sample_install != "$north_install"/* ]] || return 1
    north_value=$(trace_fact "$north_trace" data) sample_value=$(trace_fact "$sample_trace" data)
    why="data folders: North Star ${north_value:-<none>}, sample ${sample_value:-<none>}"
    [[ $north_value == "$north_home/data" && $sample_value == "$sample_home/data" ]] || return 1
    north_value=$(trace_fact "$north_trace" socket) sample_value=$(trace_fact "$sample_trace" socket)
    why="instance sockets: North Star ${north_value:-<none>}, sample ${sample_value:-<none>}"
    [[ -S $north_value && -S $sample_value && $north_value == "$north_home/"* &&
        $sample_value == "$sample_home/"* ]] || return 1
    # The lock is beside the socket, in the instance folder (yard_paths' instance_files).
    north_lock="${north_value%/*}/instance.lock" sample_lock="${sample_value%/*}/instance.lock"
    why="instance locks: North Star $north_lock, sample $sample_lock"
    [[ -f $north_lock && -f $sample_lock && $north_lock != "$sample_lock" && $north_lock == "$north_home/"* &&
        $sample_lock == "$sample_home/"* ]] || return 1
    why='a scheme is shared or not handled by its own desktop file'
    load_north_star && north_value=$scheme && default_handler &&
        load_sample && sample_value=$scheme && default_handler &&
        [[ $north_value != "$sample_value" ]] || return 1
    north_value=$(trace_fact "$north_trace" menu) sample_value=$(trace_fact "$sample_trace" menu)
    why="application menus: North Star ${north_value:-<none>}, sample ${sample_value:-<none>}"
    [[ $north_value == 'North Star' && $sample_value == yard ]] || return 1
    why="PIDs $north_pid and $sample_pid; own install folder, data folder, instance socket and lock file (each under its own home), scheme, and menu; each URL reached its own app only, re-read after ${settle}s"
}

# GPUI's headless window under North Star's config, as check_screenshot takes
# the sample's: llvmpipe shows no frames in an X11 window on the runner.
check_fresh_screenshot() {
    local out="$checks/northstar/north-star/window.png"
    load_north_star || return 1
    if (cd "$fresh" && bounded 1500 env PRODUCT_CONFIG="$fixture" "${prefix}_LIFECYCLE=info,screenshot=debug" \
        cargo run -p screenshot -- --out "$out") > "$logs/screenshot.log" 2>&1 &&
        [[ -s "$out" ]] && has_colours "$out"; then
        record PASS screenshot "GPUI headless window captured under North Star's config"
    else
        record FAIL screenshot 'missing or single-colour window.png (see northstar/north-star/screenshot.log)'
    fi
}

stop_fresh() {
    "$@" || return
    pkill -TERM -f -x "$app_pattern" 2>/dev/null || true
    sleep 1
    pkill -KILL -f -x "$app_pattern" 2>/dev/null || true
    if [[ -d "$home/logs" ]]; then cp -R "$home/logs" "$logs/app-logs"; fi
}

check_north_star() {
    local north_ready=0 sample_ready=0 running=0
    mkdir -p "$checks/northstar" || return 1
    if bounded 300 git clone --quiet --no-local "$PWD" "$fresh" > "$checks/northstar/new-app.log" 2>&1 &&
        (cd "$fresh" && bounded 900 cargo xtask new-app --name 'North Star' --identifier test.yard.northstar \
            --icon "$fresh/$northstar_icon") >> "$checks/northstar/new-app.log" 2>&1 &&
        grep -Eq '^default-members[[:space:]]*=[[:space:]]*\[[[:space:]]*"app/north-star"[[:space:]]*,?[[:space:]]*\]' \
            "$fresh/Cargo.toml" &&
        load_north_star && [[ $name == 'North Star' && $identifier == test.yard.northstar && $scheme == northstar ]]; then
        record PASS new-app 'app/north-star/ made in a fresh checkout; default-members names it'
    else
        record FAIL new-app 'see northstar/new-app.log'
        skip 'new-app failed' "${northstar_cases[@]:1}"
        return
    fi

    # No --package: the default member is the developer's application.
    bundle_fresh bundle north-star && north_ready=1
    load_sample && bundle_fresh sample-bundle sample --package yard && sample_ready=1
    load_north_star || return 1
    if (( ! north_ready )); then
        skip 'bundle failed' install desktop-name desktop-exec desktop-icon desktop-mime
    elif install_fresh install north-star; then
        check_desktop_key desktop-name Name "$name"
        check_desktop_key desktop-exec Exec "$app_binary %U"
        check_desktop_icon
        check_desktop_key desktop-mime MimeType "x-scheme-handler/$scheme;"
    else
        north_ready=0
        skip 'install failed' desktop-name desktop-exec desktop-icon desktop-mime
    fi
    if (( ! sample_ready )); then
        skip 'sample bundle failed' sample-install
    elif ! { load_sample && install_fresh sample-install sample; }; then
        sample_ready=0
    fi

    load_north_star || return 1
    if (( ! north_ready )); then
        skip 'install failed' url
    elif ! default_handler; then
        skip 'the installed desktop file is not the default scheme handler' url
    elif start_fresh_url && sleep "$settle" && in_own_trace 1; then
        running=1
        record PASS url "northstar://check reached North Star at PID $fresh_pid, refused once as no such extension"
    else
        record FAIL url 'no single North Star process with one URL refused as no such extension (see url.log)'
    fi
    check_fresh_screenshot

    if (( ! running || ! sample_ready )); then
        skip 'North Star did not run or the sample was not installed' coexistence
    elif check_fresh_coexistence; then
        record PASS coexistence "$why"
    else
        record FAIL coexistence "$why"
    fi

    stop_fresh load_north_star
    stop_fresh load_sample
    app_binary=
}

check_upstream_star() {
    local fresh="$work/update-clone"
    if check_update_repository bounded; then
        if ! bundle_fresh bundle upstream-star; then
            skip 'bundle failed' "${update_cases[@]:6}"
        elif ! install_fresh install upstream-star; then
            skip 'install failed' "${update_cases[@]:7}"
        else
            check_desktop_key desktop-name Name "$name"
            check_desktop_key desktop-exec Exec "$app_binary %U"
            check_desktop_icon
            check_desktop_key desktop-mime MimeType "x-scheme-handler/$scheme;"
            if ! default_handler; then
                skip 'the installed desktop file is not the default scheme handler' url
            elif start_fresh_url "$scheme://check" && sleep "$settle" && in_own_trace 1 &&
                [[ $(trace_counts "$trace" "$scheme") == '1 0 0' ]]; then
                record PASS url "$scheme://check reached PID $fresh_pid, refused once as no such extension"
            else
                record FAIL url 'expected one dispatch in the installed app (see update/upstream-star/url.log)'
            fi
            stop_fresh load_upstream_star
            app_binary=
        fi
    fi
}

if (( SECONDS < deadline )); then
    config=northstar
    check_north_star
fi

if (( SECONDS < deadline )); then
    config=update
    check_upstream_star
fi
