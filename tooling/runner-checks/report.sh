# shellcheck shell=bash
# shellcheck disable=SC2034,SC2154
# Each caller declares its inventory before it starts work.
failed=0

record() {
    local detail=${3//$'\r'/ }
    detail=${detail//$'\n'/ }
    if ! printf '%s %s %s: %s\n' "$1" "$config" "$2" "$detail" >> "$report"; then
        failed=1
        return 1
    fi
    printf '%s %s %s: %s\n' "$1" "$config" "$2" "$detail"
}

skip() {
    local reason=$1
    shift
    local case
    for case in "$@"; do record FAIL "$case" "skipped: $reason"; done
}

finish_report() {
    local expected case count status
    for expected in "${expected_cases[@]}"; do
        config=${expected%% *}
        case=${expected#* }
        count=$(grep -cE "^(PASS|FAIL) $config $case: " "$report")
        status=$?
        if (( status > 1 )); then
            failed=1
        elif (( count == 0 )); then
            if (( SECONDS >= deadline )); then
                record FAIL "$case" 'no result (script deadline reached)'
            else
                record FAIL "$case" 'no result'
            fi
        elif (( count != 1 )); then
            record FAIL "$case" 'duplicate results'
        fi
    done
    if (( SECONDS >= deadline )); then failed=1; fi
    grep '^FAIL '  "$report" >/dev/null
    status=$?
    if (( status != 1 )); then failed=1; fi
    printf 'DONE %s\n' "$system" >> "$report" || failed=1
    return "$failed"
}

# The fixtures use one double-quoted value per line.
value() {
    local field
    field=$(sed -nE "s/^${1}[[:space:]]*=[[:space:]]*\"([^\"]*)\".*$/\1/p" "$fixture") || return 1
    [[ $field =~ [^[:space:]] && $field != *$'\n'* ]] || return 1
    printf '%s\n' "$field"
}

read_product() {
    name=$(value name) &&
        version=$(value version) &&
        identifier=$(value identifier) &&
        scheme=$(value url_scheme)
}

trace_counts() {
    python3 "$script_dir/trace-counts.py" "$1" "$2"
}

wait_for_counts() {
    local trace_file=$1 target_scheme=$2 urls=$3 requests=$4 forwarded=$5 attempt counts
    for ((attempt=0; attempt<30 && SECONDS<deadline; attempt++)); do
        counts=$(trace_counts "$trace_file" "$target_scheme") || return 1
        if [[ "$counts" == "$urls $requests $forwarded" ]]; then return 0; fi
        sleep 1
    done
    return 1
}
