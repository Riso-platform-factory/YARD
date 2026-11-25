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

check_update_repository() {
    local checkout=$PWD ref="refs/tags/yard-runner-update-${work##*/}"
    local log_dir="$checks/update" target start stage index
    mkdir -p "$log_dir" || return 1

    for ((index=0; index<5; index++)); do
        stage=${update_cases[index]}
        if {
            case "$stage" in
                update-clone)
                    target=$(git -C "$checkout" rev-parse HEAD) &&
                        "$@" 300 git clone --quiet --no-local "$checkout" "$fresh" &&
                        git -C "$fresh" checkout -B upstream-star-app "$target~1"
                    ;;
                update-new-app)
                    # new-app resolves offline, and the job fetched the newer lockfile.
                    (cd "$fresh" && "$@" 900 cargo fetch --locked &&
                        "$@" 900 cargo xtask new-app --name 'Upstream Star' \
                        --identifier test.yard.upstreamstar --scheme upstreamstar --icon "$fresh/$northstar_icon") &&
                        git -C "$fresh" config user.name 'YARD Runner' &&
                        git -C "$fresh" config user.email runner@example.invalid &&
                        git -C "$fresh" add -- Cargo.toml Cargo.lock app/upstream-star &&
                        git -C "$fresh" commit -m 'Create Upstream Star' &&
                        start=$(git -C "$fresh" rev-parse HEAD) &&
                        load_upstream_star &&
                        [[ $name == 'Upstream Star' && $identifier == test.yard.upstreamstar && $scheme == upstreamstar ]]
                    ;;
                update)
                    (git -C "$checkout" update-ref "$ref" "$target" '' &&
                        trap 'git -C "$checkout" update-ref -d "$ref" "$target"' EXIT &&
                        cd "$fresh" && "$@" 1500 cargo xtask update --remote "$checkout" --ref "$ref" &&
                        [[ $(git branch --show-current) == "yard-update/${target:0:12}" ]])
                    ;;
                update-app-kept)
                    git -C "$fresh" diff --exit-code --quiet "$start" HEAD -- app/upstream-star
                    ;;
                update-manifest)
                    git -C "$fresh" show HEAD:Cargo.toml |
                        grep -E '^default-members[[:space:]]*=[[:space:]]*\[[[:space:]]*"app/upstream-star"[[:space:]]*,?[[:space:]]*\]'
                    ;;
            esac
        } > "$log_dir/$stage.log" 2>&1; then
            case "$stage" in
                update-clone) record PASS "$stage" "cloned YARD and checked out $target~1" ;;
                update-new-app) record PASS "$stage" "committed Upstream Star and Cargo.lock in $start" ;;
                update) record PASS "$stage" "updated to YARD $target on yard-update/${target:0:12}" ;;
                update-app-kept) record PASS "$stage" 'app/upstream-star matches the pre-update commit' ;;
                update-manifest) record PASS "$stage" 'HEAD:Cargo.toml sets app/upstream-star as the default member' ;;
            esac
        else
            record FAIL "$stage" "see update/$stage.log"
            skip "$stage failed" "${update_cases[@]:index+1}"
            return 1
        fi
    done
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
