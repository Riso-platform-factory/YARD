#!/usr/bin/env bash
set -uo pipefail
umask 077

system=macos
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=tooling/runner-checks/report.sh
source "$script_dir/report.sh" || exit 1
deadline=$((SECONDS + ${YARD_CHECK_SECONDS:-10200}))

checks="$PWD/checks/macos"
mkdir -p "$checks" || exit 1
report="$checks/report.txt"
: > "$report" || exit 1
unsigned_cases=(bundle bundle-secrets codesign plist-name plist-identifier plist-version plist-scheme install open screenshot url-cold url-warm)
expected_cases=('plain coexistence')
for config in plain spaced other; do
    for case in "${unsigned_cases[@]}"; do expected_cases+=("$config $case"); done
done
northstar_cases=(new-app bundle sample-bundle plist-name plist-identifier plist-scheme icon install sample-install
    url screenshot coexistence)
for case in "${northstar_cases[@]}"; do expected_cases+=("northstar $case"); done

signed_cases=(notarize certificate bundle bundle-secrets app-identity entitlements dmg-identity
    keychain interrupt interrupt-secrets interrupt-keychain notarize-missing notarize-missing-secrets trap-keychain)
for case in "${signed_cases[@]}"; do expected_cases+=("signed $case"); done

work=$(mktemp -d) || exit 1
work=$(cd "$work" && pwd -P)
export YARD_HOME="$work/yard-home"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
unset MACOS_CERTIFICATE MACOS_CERTIFICATE_PASSWORD WINDOWS_CERTIFICATE WINDOWS_CERTIFICATE_PASSWORD
unset APPLE_NOTARIZATION_KEY APPLE_NOTARIZATION_KEY_ID APPLE_NOTARIZATION_ISSUER_ID
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
# What every launch sets in the product's lifecycle variable, and the host of
# every URL sent (no extension has it, so the shell refuses each URL).
lifecycle_filter='info,yard_shell=debug'
url_host=nobody

stop_app() {
    if [[ -n ${app_pattern:-} ]]; then
        local attempt
        pkill -f -x "$app_pattern" 2>/dev/null || true
        for ((attempt=0; attempt<5; attempt++)); do
            if ! pgrep -f -x "$app_pattern" >/dev/null; then break; fi
            sleep 1
        done
        pkill -KILL -f -x "$app_pattern" 2>/dev/null || true
    fi
    app_pattern=
}

cleanup() {
    local path paths search_list deleted=ok inspected=ok
    stop_app
    if [[ -f "$work/apps" ]]; then
        while IFS= read -r path; do
            pattern_for_app "$path"
            stop_app
            run_bounded cleanup 10 "$lsregister" -u "$path" >/dev/null 2>&1 || true
            if [[ $path == "$HOME/Applications/"* ]]; then
                rm -rf "$path"
                rmdir "${path%/*}" || failed=1
            fi
        done < "$work/apps"
    fi
    config=signed
    if paths=$(find "$work" -name 'signing.keychain*' -type f); then
        while IFS= read -r path; do
            [[ -n "$path" ]] || continue
            if ! run_bounded cleanup 15 security delete-keychain "$path" >/dev/null 2>&1; then
                deleted=failed
            fi
        done <<< "$paths"
    else
        deleted=inspection-failed
    fi
    if ! search_list=$(run_bounded cleanup 15 security list-keychains -d user) ||
        [[ "$search_list" == *"$work/"* ]]; then
        inspected=failed
    fi
    if [[ $deleted == ok && $inspected == ok ]]; then
        record PASS trap-keychain 'fallback deletion (if needed) and search-list inspection succeeded'
    else
        record FAIL trap-keychain "fallback deletion: $deleted; search-list inspection: $inspected"
    fi
    rm -rf "$work"
}
trap 'if (( $? != 0 )); then failed=1; fi; cleanup; finish_report; exit $?' EXIT
trap 'failed=1; exit 1' INT TERM

# Start in a separate process group with SIGINT enabled even when Bash is
# noninteractive. In interrupt mode wait for xtask's real signing step.
run_bounded() {
    local mode=$1 limit=$2 remaining=$((deadline - SECONDS))
    shift 2
    if [[ $mode != cleanup ]]; then
        if (( remaining <= 0 )); then return 124; fi
        if (( limit > remaining )); then limit=$remaining; fi
    fi
    python3 - "$mode" "$limit" "$@" <<'PY'
import os
import pathlib
import signal
import subprocess
import sys
import time

mode, limit, *command = sys.argv[1:]
with subprocess.Popen(command, start_new_session=True) as process:
    try:
        deadline = time.monotonic() + float(limit)
        if mode == "interrupt":
            log = pathlib.Path(os.environ["BUNDLE_OUTPUT"])
            private = pathlib.Path(os.environ["TMPDIR"])
            while process.poll() is None and time.monotonic() < deadline:
                if b"sign" in log.read_bytes().splitlines() and list(private.glob("yard-signing.*")):
                    process.send_signal(signal.SIGINT)
                    print("sent SIGINT during signing", flush=True)
                    deadline = min(deadline, time.monotonic() + 90)
                    break
                time.sleep(0.02)
            else:
                raise subprocess.TimeoutExpired(command, limit)
        status = process.wait(timeout=max(0, deadline - time.monotonic()))
        sys.exit(status if status >= 0 else 128 - status)
    except subprocess.TimeoutExpired:
        print("command timed out or exited before the interrupt checkpoint", file=sys.stderr, flush=True)
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
        sys.exit(124)
PY
}

run_bundle() {
    local label=$1 mode=$2
    shift 2
    bundle_status=0
    bundle_log="$work/$config/$label.raw.log"
    local command=(cargo xtask)
    if [[ $mode == interrupt ]]; then
        # Signal xtask itself, rather than the Cargo alias's parent process.
        command=("$CARGO_TARGET_DIR/debug/xtask")
    fi
    rm -f "$CARGO_TARGET_DIR"/{xtask.log,xtask.log.old,lifecycle.jsonl}
    mkdir -p "$work/$config" || return 1
    # The interrupt monitor reads the child output while it is being written.
    # shellcheck disable=SC2094
    BUNDLE_OUTPUT="$bundle_log" run_bounded "$mode" 1500 "${command[@]}" bundle macos --package yard "$@" \
        > "$bundle_log" 2>&1 || bundle_status=$?

    local files=("$bundle_log") file
    for file in xtask.log xtask.log.old lifecycle.jsonl; do
        if [[ -f "$CARGO_TARGET_DIR/$file" ]]; then
            files+=("$CARGO_TARGET_DIR/$file")
        fi
    done
    local pattern_file="$work/secret-patterns" grep_status=1
    if [[ -n ${MACOS_CERTIFICATE:-} ]]; then
        if printf '%s\n' "$MACOS_CERTIFICATE_PASSWORD" "${MACOS_CERTIFICATE:0:40}" > "$pattern_file"; then
            grep -F -f "$pattern_file" "${files[@]}" >/dev/null
            grep_status=$?
        else
            grep_status=2
        fi
        rm -f "$pattern_file"
    fi
    if (( bundle_status == 124 || grep_status != 1 )); then
        record FAIL "$label-secrets" 'bundle timed out, secret matched, or scan failed; logs withheld'
        bundle_status=1
    else
        if cp "$bundle_log" "$logs/$label.log"; then
            record PASS "$label-secrets" 'no signing credentials found in bundle logs'
            for file in "${files[@]:1}"; do cp "$file" "$logs/$label-${file##*/}" || failed=1; done
        else
            record FAIL "$label-secrets" 'could not publish scanned bundle logs'
        fi
    fi
}

check_bundle() {
    run_bundle bundle normal --config "$fixture" --out "$dist"
    if (( bundle_status == 0 )) && [[ -d "$app" ]]; then
        record PASS bundle "created $name.app"
        return 0
    fi
    record FAIL bundle "exit $bundle_status or app missing (see $config/bundle.log)"
    return 1
}

check_codesign() {
    if run_bounded normal 30 codesign --verify --deep --strict "$app" > "$logs/verify.log" 2>&1; then
        record PASS codesign 'deep, strict verification succeeded'
    else
        record FAIL codesign "see $config/verify.log"
    fi
}

check_plist() {
    local key=$1 expected=$2 actual
    if actual=$(plutil -extract "$key" raw -o - "$app/Contents/Info.plist" 2>> "$logs/plist.log") &&
        [[ "$actual" == "$expected" ]]; then
        record PASS "$3" "$key = $actual"
    else
        record FAIL "$3" "$key expected $expected, got ${actual:-<missing>}"
    fi
}

pattern_for_app() {
    local path=$1
    app_pattern=$(printf '%s' "$path/Contents/MacOS/$(basename "$path" .app)" |
        sed 's/[][\\.^$*+?(){}|]/\\&/g')
}

check_install() {
    local case=${1:-install} install_dir
    if mkdir -p "$HOME/Applications" &&
        install_dir=$(mktemp -d "$HOME/Applications/yard-runner.XXXXXX") &&
        installed="$install_dir/$name.app" &&
        printf '%s\n' "$installed" >> "$work/apps" &&
        run_bounded normal 60 ditto "$app" "$installed" > "$logs/install.log" 2>&1 &&
        run_bounded normal 15 "$lsregister" -f "$installed" >> "$logs/install.log" 2>&1; then
        record PASS "$case" 'installed under ~/Applications and registered with lsregister -f'
        return 0
    fi
    record FAIL "$case" "see $config/install.log"
    return 1
}

wait_for_window() {
    local attempt id
    for ((attempt=0; attempt<30 && SECONDS<deadline; attempt++)); do
        id=$(sed -nE 's/.*"kind":"group-start",.*"id":([0-9]+),.*"name":"window-open".*/\1/p' "$trace" 2>/dev/null | tail -n 1)
        if [[ -n "$id" ]] && grep -Eq "\"kind\":\"group-end\",.*\"id\":$id,\"outcome\":\"success\"" "$trace"; then
            return 0
        fi
        sleep 1
    done
    return 1
}

launch_app() {
    pattern_for_app "$installed"
    run_bounded normal 15 open --env "${prefix}_HOME=$home" \
        --env "${prefix}_LIFECYCLE=$lifecycle_filter" "$installed" > "$logs/open.log" 2>&1 &&
        wait_for_window && pgrep -f -x "$app_pattern" >/dev/null
}

check_open() {
    if launch_app; then
        record PASS open 'the installed app opened a window'
        return 0
    fi
    record FAIL open 'no window from the installed app (see open.log and app logs)'
    return 1
}

check_screenshot() {
    if run_bounded normal 15 screencapture -x "$logs/window.png" > "$logs/screenshot.log" 2>&1 &&
        [[ -s "$logs/window.png" ]]; then
        record PASS screenshot 'captured the screen with the installed window'
    else
        record FAIL screenshot "see $config/screenshot.log"
    fi
}

handler_is_installed() {
    local handler
    handler=$(run_bounded normal 5 osascript -l JavaScript -e '
ObjC.import("AppKit");
function run(args) {
    const url = $.NSURL.URLWithString(args[0] + "://nobody/check");
    const app = $.NSWorkspace.sharedWorkspace.URLForApplicationToOpenURL(url);
    if (app == null || app.isNil()) return "";
    const path = ObjC.unwrap(app.URLByResolvingSymlinksInPath.path);
    return path == null ? "" : path;
}
' "$scheme" 2>> "$logs/url.log") || return 1
    printf 'resolved handler: %s\n' "${handler:-<none>}" >> "$logs/url.log"
    [[ "$handler" == "$installed" ]]
}

wait_for_handler() {
    local attempt
    for ((attempt=0; attempt<5 && SECONDS<deadline; attempt++)); do
        if handler_is_installed; then return 0; fi
        sleep 1
    done
    return 1
}

register_handler() {
    registration='installed under ~/Applications, ran lsregister -f, and polled Launch Services'
    if wait_for_handler; then return 0; fi

    registration="$registration, then tried lsregister -R -f"
    if run_bounded normal 10 "$lsregister" -R -f "$installed" >> "$logs/url.log" 2>&1 &&
        wait_for_handler; then return 0; fi

    registration="$registration and LSSetDefaultHandlerForURLScheme"
    # shellcheck disable=SC2016
    if run_bounded normal 10 osascript -l JavaScript -e '
ObjC.import("CoreServices");
function run(args) {
    const result = $.LSSetDefaultHandlerForURLScheme($(args[0]), $(args[1]));
    if (result !== 0) throw Error("LSSetDefaultHandlerForURLScheme returned " + result);
}
' "$scheme" "$identifier" >> "$logs/url.log" 2>&1 && wait_for_handler; then return; fi
    return 1
}

send_url() {
    local phase=$1 expected_pid=${2:-} counts urls requests forwarded pid url
    handler_is_installed || return 1
    pid=$(pgrep -f -x "$app_pattern") || return 1
    [[ $pid =~ ^[0-9]+$ ]] || return 1
    counts=$(trace_counts "$trace" "$scheme") || return 1
    if [[ -n "$expected_pid" && ( "$pid" != "$expected_pid" || "$counts" != '1 0 0' ) ]]; then
        return 1
    fi
    read -r urls requests forwarded <<< "$counts"
    url="$scheme://$url_host/$phase-$RANDOM-$RANDOM"
    run_bounded normal 15 open "$url" >> "$logs/url.log" 2>&1 &&
        wait_for_counts "$trace" "$scheme" "$((urls + 1))" "$requests" "$forwarded" &&
        [[ $(pgrep -f -x "$app_pattern") == "$pid" ]]
}

check_cold_url() {
    local pid
    if pgrep -f -x "$app_pattern" >/dev/null; then
        return 1
    elif [[ $? != 1 ]]; then
        return 1
    fi
    handler_is_installed && rm -f "$trace" &&
        run_bounded normal 15 open --env "${prefix}_HOME=$home" \
            --env "${prefix}_LIFECYCLE=$lifecycle_filter" \
            "$scheme://$url_host/cold-$RANDOM-$RANDOM" >> "$logs/url.log" 2>&1 &&
        wait_for_counts "$trace" "$scheme" 1 0 0 &&
        pid=$(pgrep -f -x "$app_pattern") && [[ $pid =~ ^[0-9]+$ ]] &&
        printf '%s\n' "$pid"
}

check_urls() {
    local cold_pid
    if ! register_handler; then
        skip "Launch Services did not resolve the installed app ($registration; see url.log)" url-cold url-warm
        return
    fi
    pattern_for_app "$installed"
    if ! cold_pid=$(check_cold_url); then
        record FAIL url-cold "expected a new process with one URL dispatch ($registration; see url.log)"
        skip 'cold launch failed' url-warm
        return
    fi
    record PASS url-cold "the URL launched PID $cold_pid and dispatched once ($registration)"
    if send_url warm "$cold_pid"; then
        record PASS url-warm "PID $cold_pid dispatched one more URL ($registration)"
    else
        record FAIL url-warm "expected one more URL dispatch in the original process ($registration; see url.log)"
    fi
}

load_config() {
    config=$1
    fixture="$PWD/crates/yard_build_config/tests/configs/$config/product.toml"
    read_product || return 1
    prefix=$(printf '%s' "$name" | LC_ALL=C tr '[:lower:]' '[:upper:]' | sed 's/[^A-Z0-9]/_/g')
    logs="$checks/$config"
    home="$work/$config/home"
    trace="$home/logs/lifecycle.jsonl"
    dist="$work/$config/dist"
    app="$dist/$name.app"
    mkdir -p "$logs" "$home"
}

for fixture_name in plain spaced other; do
    if (( SECONDS >= deadline )); then break; fi
    if ! load_config "$fixture_name"; then
        skip 'missing or blank required product field' "${unsigned_cases[@]}"
        continue
    fi
    if ! check_bundle; then
        skip 'bundle failed' codesign plist-name plist-identifier plist-version plist-scheme install open screenshot url-cold url-warm
        continue
    fi
    check_codesign
    check_plist CFBundleName "$name" plist-name
    check_plist CFBundleIdentifier "$identifier" plist-identifier
    check_plist CFBundleShortVersionString "$version" plist-version
    check_plist CFBundleURLTypes.0.CFBundleURLSchemes.0 "$scheme" plist-scheme
    if check_install; then
        printf '%s\n' "$installed" > "$work/$config/installed-path"
        if check_open; then
            check_screenshot
        else
            skip 'open failed' screenshot
        fi
        stop_app
        check_urls
    else
        skip 'install failed' open screenshot url-cold url-warm
    fi
    stop_app
    if [[ -d "$home/logs" ]]; then cp -R "$home/logs" "$logs/app-logs" || failed=1; fi
done

check_coexistence() {
    local product plain_trace plain_pattern plain_pid other_trace other_pid
    local plain_before other_before plain_route
    for product in plain other; do
        load_config "$product" || return 1
        installed=$(cat "$work/$product/installed-path") || return 1
        rm -f "$trace" || return 1
        launch_app || return 1
    done
    other_trace=$trace
    other_pid=$(pgrep -f -x "$app_pattern") || return 1
    load_config plain || return 1
    installed=$(cat "$work/plain/installed-path") || return 1
    pattern_for_app "$installed"
    plain_trace=$trace plain_pattern=$app_pattern
    plain_pid=$(pgrep -f -x "$plain_pattern") || return 1
    [[ $plain_pid =~ ^[0-9]+$ && $other_pid =~ ^[0-9]+$ && $plain_pid != "$other_pid" ]] || return 1

    other_before=$(trace_counts "$other_trace" "*") || return 1
    registration='Launch Services'
    send_url coexist-plain &&
        [[ $(trace_counts "$other_trace" "*") == "$other_before" ]] || return 1
    plain_route=$registration
    plain_before=$(trace_counts "$plain_trace" "*") || return 1
    load_config other || return 1
    installed=$(cat "$work/other/installed-path") || return 1
    pattern_for_app "$installed"
    registration='Launch Services'
    send_url coexist-other &&
        [[ $(trace_counts "$plain_trace" "*") == "$plain_before" ]] &&
        [[ $(pgrep -f -x "$plain_pattern") == "$plain_pid" && $(pgrep -f -x "$app_pattern") == "$other_pid" ]] || return 1
    cp "$plain_trace" "$checks/plain/coexist-trace.jsonl" &&
        cp "$other_trace" "$checks/other/coexist-trace.jsonl" || return 1
    config=plain
    record PASS coexistence "each URL reached its own app's URL handler only (plain: $plain_route; other: $registration)"
}

if (( SECONDS < deadline )) && ! check_coexistence; then
    config=plain
    record FAIL coexistence 'expected two running apps and isolated URL delivery'
fi

# A developer's application, made with `cargo xtask new-app` in a fresh
# checkout, installed beside the sample bundled there with `--package yard`.
# Each starts with its own home and lifecycle variables.
fresh="$work/fresh"
northstar_icon=crates/yard_build_config/tests/configs/other/icon.png
# Seconds to wait before counts are read again, to catch a late delivery.
settle=3

trace_fact() {
    python3 "$script_dir/trace-facts.py" "$1" "$2"
}

# Loads the fresh checkout's product from app/$1/, with its files under $2.
load_fresh() {
    fixture="$fresh/app/$1/product.toml"
    read_product || return 1
    prefix=$(printf '%s' "$name" | LC_ALL=C tr '[:lower:]' '[:upper:]' | sed 's/[^A-Z0-9]/_/g')
    logs="$checks/northstar/$2"
    home="$work/northstar/$2/home"
    trace="$home/logs/lifecycle.jsonl"
    dist="$work/northstar/$2/dist"
    app="$dist/$name.app"
    installed_path="$work/northstar/$2/installed-path"
    installed=$(cat "$installed_path" 2>/dev/null) || installed=
    mkdir -p "$logs" "$home"
}

load_north_star() { load_fresh north-star north-star; }
load_sample() { load_fresh yard sample; }

bundle_fresh() {
    local case=$1 label=$2 status=0 file
    shift 2
    rm -f "${CARGO_TARGET_DIR:?}"/{xtask.log,xtask.log.old,lifecycle.jsonl}
    (cd "$fresh" && run_bounded normal 1500 cargo xtask bundle macos "$@" --out "$dist") \
        > "$logs/bundle.log" 2>&1 || status=$?
    for file in xtask.log xtask.log.old lifecycle.jsonl; do
        if [[ -f "$CARGO_TARGET_DIR/$file" ]]; then cp "$CARGO_TARGET_DIR/$file" "$logs/bundle-$file"; fi
    done
    if (( status == 0 )) && [[ -d "$app" ]]; then
        record PASS "$case" "created $name.app"
        return 0
    fi
    record FAIL "$case" "exit $status or app missing (see northstar/$label/bundle.log)"
    return 1
}

# The app's icon is the one given to new-app: the same icns as Other Lab's,
# bundled earlier in this run from that icon, and not the sample's.
check_fresh_icon() {
    local icon_file reference="$work/other/dist/Other Lab.app/Contents/Resources/Other Lab.icns"
    local sample_icns="$work/northstar/sample/dist/yard.app/Contents/Resources/yard.icns"
    if icon_file=$(plutil -extract CFBundleIconFile raw -o - "$app/Contents/Info.plist" 2>> "$logs/plist.log") &&
        [[ $icon_file == "$name.icns" ]] && cmp -s "$app/Contents/Resources/$icon_file" "$reference" &&
        ! cmp -s "$app/Contents/Resources/$icon_file" "$sample_icns"; then
        record PASS icon "CFBundleIconFile = $icon_file, made from the icon given to new-app"
    else
        record FAIL icon "CFBundleIconFile ${icon_file:-<missing>} is not the icns of the icon given to new-app (compared with Other Lab.app's)"
    fi
}

# Every scheme of every CFBundleURLTypes entry of the app at $1, sorted and
# without repeats, on one line.
url_schemes() {
    python3 - "$1/Contents/Info.plist" <<'PY'
import plistlib
import sys

with open(sys.argv[1], "rb") as file:
    plist = plistlib.load(file)
schemes = set()
for entry in plist.get("CFBundleURLTypes", []):
    schemes.update(entry.get("CFBundleURLSchemes", []))
print(" ".join(sorted(schemes)))
PY
}

check_url_schemes() {
    local actual
    if actual=$(url_schemes "$app" 2>> "$logs/plist.log") && [[ $actual == "$scheme" ]]; then
        record PASS plist-scheme "the CFBundleURLTypes entries declare exactly {$actual}"
    else
        record FAIL plist-scheme "the CFBundleURLTypes entries declare {${actual:-}}, expected exactly {$scheme}"
    fi
}

# The loaded product's trace names it, and holds exactly $1 URLs refused as
# `no such extension`.
in_own_trace() {
    [[ $(trace_fact "$trace" name) == "$name" &&
        $(trace_fact "$trace" identifier) == "$identifier" &&
        $(trace_fact "$trace" no-such-extension) == "$1" ]]
}

check_fresh_coexistence() {
    local north_home north_trace north_installed north_pattern north_pid north_identifier
    local sample_home sample_trace sample_installed sample_pattern sample_pid sample_identifier sample_before north_before
    local north_value sample_value
    load_north_star && pattern_for_app "$installed" || return 1
    north_home=$home north_trace=$trace north_installed=$installed north_pattern=$app_pattern
    north_identifier=$identifier
    why='the sample did not start from yard://check with one URL refused as no such extension, or North Star saw it'
    north_before=$(trace_counts "$north_trace" "*") &&
        load_sample && register_handler && pattern_for_app "$installed" && sample_pid=$(check_cold_url) &&
        sleep "$settle" && in_own_trace 1 && [[ $(trace_counts "$north_trace" "*") == "$north_before" ]] || return 1
    sample_home=$home sample_trace=$trace sample_installed=$installed sample_pattern=$app_pattern
    sample_identifier=$identifier
    why='expected one North Star and one sample process'
    north_pid=$(pgrep -f -x "$north_pattern") && [[ $north_pid =~ ^[0-9]+$ && $sample_pid =~ ^[0-9]+$ &&
        $north_pid != "$sample_pid" ]] || return 1

    # Each delivery is sampled again after a pause: the destination still has
    # exactly one more URL, and the peer none.
    why="northstar://check did not reach North Star alone (re-read after ${settle}s)"
    load_north_star && pattern_for_app "$installed" && sample_before=$(trace_counts "$sample_trace" "*") &&
        send_url coexist-north && sleep "$settle" && in_own_trace 2 &&
        [[ $(trace_counts "$sample_trace" "*") == "$sample_before" ]] || return 1
    why="yard://check did not reach the sample alone (re-read after ${settle}s)"
    load_sample && pattern_for_app "$installed" && north_before=$(trace_counts "$north_trace" "*") &&
        send_url coexist-sample && sleep "$settle" && in_own_trace 2 &&
        [[ $(trace_counts "$north_trace" "*") == "$north_before" ]] || return 1
    why='a process was replaced while URLs were delivered'
    [[ $(pgrep -f -x "$north_pattern") == "$north_pid" && $(pgrep -f -x "$sample_pattern") == "$sample_pid" ]] ||
        return 1

    why="install folders meet: $north_installed and $sample_installed"
    [[ $north_installed != "$sample_installed" && ${north_installed%/*} != "${sample_installed%/*}" &&
        $north_installed != "$sample_installed"/* && $sample_installed != "$north_installed"/* ]] || return 1
    north_value=$(trace_fact "$north_trace" data) sample_value=$(trace_fact "$sample_trace" data)
    why="data folders: North Star ${north_value:-<none>}, sample ${sample_value:-<none>}"
    [[ $north_value == "$north_home/data" && $sample_value == "$sample_home/data" ]] || return 1
    # macOS keeps one instance per bundle identifier, through Launch Services.
    why="bundle identifiers: North Star $north_identifier, sample $sample_identifier"
    north_value=$(plutil -extract CFBundleIdentifier raw -o - "$north_installed/Contents/Info.plist") &&
        sample_value=$(plutil -extract CFBundleIdentifier raw -o - "$sample_installed/Contents/Info.plist") &&
        [[ $north_value == "$north_identifier" && $sample_value == "$sample_identifier" &&
            $north_value != "$sample_value" ]] || return 1
    north_value=$(url_schemes "$north_installed") sample_value=$(url_schemes "$sample_installed")
    why="installed URL schemes: North Star {$north_value}, sample {$sample_value}; expected exactly {northstar} and {yard}"
    [[ $north_value == northstar && $sample_value == yard ]] || return 1
    why='a scheme is not resolved to its own installed app'
    load_north_star && handler_is_installed && load_sample && handler_is_installed || return 1
    north_value=$(trace_fact "$north_trace" menu) sample_value=$(trace_fact "$sample_trace" menu)
    why="application menus: North Star ${north_value:-<none>}, sample ${sample_value:-<none>}"
    [[ $north_value == 'North Star' && $sample_value == yard ]] || return 1
    why="PIDs $north_pid and $sample_pid; own install folder, data folder, bundle identifier, URL schemes (exactly {northstar} and {yard}), and menu; each URL reached its own app only, re-read after ${settle}s"
}

stop_fresh() {
    "$@" || return
    if [[ -n "$installed" ]]; then
        pattern_for_app "$installed"
        stop_app
    fi
    if [[ -d "$home/logs" ]]; then cp -R "$home/logs" "$logs/app-logs" || failed=1; fi
}

check_north_star() {
    local north_ready=0 sample_ready=0 running=0 north_pid
    mkdir -p "$checks/northstar" || return 1
    if run_bounded normal 300 git clone --quiet --no-local "$PWD" "$fresh" > "$checks/northstar/new-app.log" 2>&1 &&
        (cd "$fresh" && run_bounded normal 900 cargo xtask new-app --name 'North Star' \
            --identifier test.yard.northstar --icon "$fresh/$northstar_icon") >> "$checks/northstar/new-app.log" 2>&1 &&
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
    if (( north_ready )); then
        check_plist CFBundleName "$name" plist-name
        check_plist CFBundleIdentifier "$identifier" plist-identifier
        check_url_schemes
        check_fresh_icon
        if check_install; then
            printf '%s\n' "$installed" > "$installed_path" || return 1
        else
            north_ready=0
        fi
    else
        skip 'bundle failed' plist-name plist-identifier plist-scheme icon install
    fi
    if (( ! sample_ready )); then
        skip 'sample bundle failed' sample-install
    elif load_sample && check_install sample-install; then
        printf '%s\n' "$installed" > "$installed_path" || return 1
    else
        sample_ready=0
    fi

    load_north_star || return 1
    if (( ! north_ready )); then
        skip 'install failed' url screenshot
    elif ! register_handler; then
        skip "Launch Services did not resolve the installed app ($registration; see url.log)" url screenshot
    elif pattern_for_app "$installed" && north_pid=$(check_cold_url) && sleep "$settle" && in_own_trace 1; then
        running=1
        record PASS url "northstar://check launched PID $north_pid, refused once as no such extension ($registration)"
        check_screenshot
    else
        record FAIL url "expected a new North Star process with one URL refused as no such extension (see url.log)"
        skip 'url failed' screenshot
    fi

    if (( ! running || ! sample_ready )); then
        skip 'North Star did not run or the sample was not installed' coexistence
    elif check_fresh_coexistence; then
        record PASS coexistence "$why"
    else
        record FAIL coexistence "$why"
    fi

    stop_fresh load_north_star
    stop_fresh load_sample
}

if (( SECONDS < deadline )); then
    config=northstar
    lifecycle_filter='info,yard_shell=debug,yard_paths=debug'
    url_host=check
    check_north_star
    lifecycle_filter='info,yard_shell=debug'
    url_host=nobody
    # The signed checks run xtask's binary itself: build the workspace's own
    # again over the fresh checkout's, which shares the target folder.
    run_bounded normal 900 cargo build -p xtask > "$checks/northstar/xtask-rebuild.log" 2>&1 ||
        printf 'could not rebuild xtask for the signed checks (see northstar/xtask-rebuild.log)\n' >&2
fi

make_identity() {
    local identity_dir=$1
    mkdir -p "$identity_dir" || return 1
    cat > "$identity_dir/identity.cnf" <<CONF
[req]
distinguished_name = dn
prompt = no
[dn]
CN = $identity
OU = ${identity##*(}
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CONF
    # The organizational unit in the test fixture is the team ID in parentheses.
    sed -i '' '/^OU = /s/)$//' "$identity_dir/identity.cnf" || return 1
    MACOS_CERTIFICATE_PASSWORD=$(run_bounded normal 10 openssl rand -hex 24) || return 1
    export MACOS_CERTIFICATE_PASSWORD
    run_bounded normal 30 openssl req -x509 -newkey rsa:2048 -nodes -days 2 -config "$identity_dir/identity.cnf" \
        -extensions ext -keyout "$identity_dir/key.pem" -out "$identity_dir/certificate.pem" || return 1
    local export_args=(pkcs12 -export)
    if [[ $(openssl version) == OpenSSL\ [3-9]* ]]; then export_args+=(-legacy); fi
    run_bounded normal 30 openssl "${export_args[@]}" -inkey "$identity_dir/key.pem" -in "$identity_dir/certificate.pem" \
        -out "$identity_dir/identity.p12" -passout env:MACOS_CERTIFICATE_PASSWORD || return 1
    MACOS_CERTIFICATE=$(base64 < "$identity_dir/identity.p12" | tr -d '\n') || return 1
    export MACOS_CERTIFICATE
}

check_app_identity() {
    if run_bounded normal 30 codesign -dv --verbose=4 "$app" > "$logs/app-signature.log" 2>&1 &&
        grep -Fxq "Authority=$identity" "$logs/app-signature.log" &&
        grep -Eq 'flags=.*\([^)]*runtime' "$logs/app-signature.log"; then
        record PASS app-identity 'test authority and hardened runtime present'
    else
        record FAIL app-identity 'expected authority or runtime missing (see app-signature.log)'
    fi
}

check_entitlements() {
    if run_bounded normal 30 codesign -d --entitlements - --xml "$app" > "$logs/entitlements.plist" 2> "$logs/entitlements.log" &&
        python3 - "$entitlements" "$logs/entitlements.plist" <<'PY'
import plistlib
import sys

with open(sys.argv[1], "rb") as expected, open(sys.argv[2], "rb") as actual:
    if plistlib.load(expected) != plistlib.load(actual):
        sys.exit("signed entitlements differ from the config")
PY
    then
        record PASS entitlements 'signed entitlements match the config'
    else
        record FAIL entitlements 'could not read matching entitlements'
    fi
}

check_dmg() {
    if run_bounded normal 30 codesign --verify "$dist/$name-$version.dmg" > "$logs/dmg-verify.log" 2>&1 &&
        run_bounded normal 30 codesign -dv --verbose=4 "$dist/$name-$version.dmg" > "$logs/dmg-signature.log" 2>&1 &&
        grep -Fxq "Authority=$identity" "$logs/dmg-signature.log" &&
        grep -Fxq "Identifier=$identifier.dmg" "$logs/dmg-signature.log"; then
        record PASS dmg-identity 'DMG signature verified with the test authority and configured identifier'
    else
        record FAIL dmg-identity 'see dmg-signature.log'
    fi
}

check_keychain_cleanup() {
    local private=$1 label=$2 search_list leftover
    if leftover=$(find "$private" -name 'yard-signing.*' -print) &&
        search_list=$(run_bounded normal 15 security list-keychains -d user) &&
        [[ -z "$leftover" && "$search_list" != *"$private/"* ]]; then
        record PASS "$label" 'temporary signing folder gone and keychain absent from search list'
    else
        record FAIL "$label" 'temporary signing folder or search entry remains, or search failed'
    fi
}

check_missing_notarization() {
    run_bundle notarize-missing normal --config "$fixture" --out "$work/signed/notarize-out"
    local message='notarize = true needs APPLE_NOTARIZATION_KEY, APPLE_NOTARIZATION_KEY_ID, APPLE_NOTARIZATION_ISSUER_ID in the environment'
    if (( bundle_status == 1 )) && grep -Fq "$message" "$bundle_log" && [[ ! -e "$work/signed/notarize-out" ]]; then
        record PASS notarize-missing 'xtask refused before building and named all three missing variables'
    else
        record FAIL notarize-missing "expected the missing-credentials refusal, got exit $bundle_status (see notarize-missing.log)"
    fi
}

check_interrupted_signing() {
    local private="$work/signed/interrupt-tmp" interrupted_out="$work/signed/interrupt-out"
    mkdir -p "$private"
    TMPDIR="$private" run_bundle interrupt interrupt --config "$signing_fixture" --out "$interrupted_out"
    if (( bundle_status == 130 )) && grep -Fxq 'sent SIGINT during signing' "$bundle_log" &&
        [[ ! -e "$interrupted_out/$name.app" && ! -e "$interrupted_out/$name-$version.dmg" ]]; then
        record PASS interrupt 'SIGINT after signing began returned 130 and published nothing'
    else
        record FAIL interrupt "expected exit 130 and no publication, got exit $bundle_status (see interrupt.log)"
    fi
    check_keychain_cleanup "$private" interrupt-keychain
}

if (( SECONDS >= deadline )); then exit; fi
if ! load_config signed || ! identity=$(value identity) || ! entitlements_file=$(value entitlements); then
    for case in "${signed_cases[@]}"; do
        if [[ $case != trap-keychain ]]; then record FAIL "$case" 'skipped: missing or blank required product field'; fi
    done
    exit 1
fi
entitlements="${fixture%/*}/$entitlements_file"
record PASS notarize "not run on the runner (needs the publisher's Apple account)"
signing_fixture="$work/signed/config/product.toml"
private="$work/signed/normal-tmp"
mkdir -p "$private"
if make_identity "$work/signed/identity" > "$logs/certificate.log" 2>&1 &&
    run_bounded normal 30 ditto "${fixture%/*}" "${signing_fixture%/*}" &&
    sed -i '' 's/^notarize = true$/notarize = false/' "$signing_fixture"; then
    record PASS certificate 'created a temporary self-signed code-signing identity'
    TMPDIR="$private" run_bundle bundle normal --config "$signing_fixture" --out "$dist"
    if (( bundle_status == 0 )) && [[ -d "$app" ]]; then
        record PASS bundle "created signed $name.app"
        check_app_identity
        check_entitlements
        check_dmg
    else
        record FAIL bundle "exit $bundle_status or app missing (see signed/bundle.log)"
        skip 'signed bundle failed' app-identity entitlements dmg-identity
    fi
    check_keychain_cleanup "$private" keychain
    check_interrupted_signing
else
    record FAIL certificate 'could not prepare the identity and signing config'
    skip 'signing setup failed' bundle bundle-secrets app-identity entitlements dmg-identity keychain interrupt interrupt-secrets interrupt-keychain
fi
check_missing_notarization

