#!/bin/sh
# labeldeck installer for Linux and macOS.
#
# Downloads the correct release archive for this platform, verifies its
# SHA-256, executes the candidate binary's --version as a smoke test,
# and only then replaces any existing installation atomically.
set -eu

die() {
    printf 'error: %s\n' "$1" >&2
    return 1
}

detect_arch() {
    os=$1
    arch=$(uname -m)
    case "$os:$arch" in
        Linux:x86_64|Linux:amd64|Darwin:x86_64) printf '%s\n' x86_64 ;;
        Linux:aarch64|Linux:arm64|Darwin:arm64) printf '%s\n' aarch64 ;;
        *) die "unsupported platform: $os/$arch" ;;
    esac
}

glibc_is_supported() {
    version=$1
    major=${version%%.*}
    minor=${version#*.}
    test "$major" -gt 2 || { test "$major" -eq 2 && test "$minor" -ge 28; }
}

detect_libc() {
    if command -v getconf >/dev/null 2>&1; then
        getconf_output=$(getconf GNU_LIBC_VERSION 2>/dev/null || :)
        version=$(printf '%s\n' "$getconf_output" | sed -n \
            's/^glibc \([0-9][0-9]*\.[0-9][0-9]*\)$/\1/p')
        if test -n "$version"; then
            select_glibc "$version"
            return
        fi
    fi

    if command -v ldd >/dev/null 2>&1; then
        ldd_output=$(ldd --version 2>&1 || :)
        case "$ldd_output" in
            *musl*|*Musl*) printf '%s\n' musl; return ;;
            *'GNU libc'*|*GLIBC*|*'GNU C Library'*)
                version=$(printf '%s\n' "$ldd_output" | sed -n \
                    '1s/.* \([0-9][0-9]*\.[0-9][0-9]*\)$/\1/p')
                if test -n "$version"; then
                    select_glibc "$version"
                    return
                fi
                ;;
        esac
    fi

    die 'unable to detect libc; set LABELDECK_LIBC=gnu or LABELDECK_LIBC=musl'
}

select_glibc() {
    version=$1
    if glibc_is_supported "$version"; then
        printf '%s\n' gnu
    else
        printf 'Detected glibc %s, older than labeldeck minimum 2.28; using static musl build.\n' \
            "$version" >&2
        printf '%s\n' musl
    fi
}

select_libc() {
    case "${LABELDECK_LIBC:-}" in
        gnu|musl) printf '%s\n' "$LABELDECK_LIBC" ;;
        '') detect_libc ;;
        *) die 'LABELDECK_LIBC must be gnu or musl' ;;
    esac
}

detect_target() {
    os=$(uname -s)
    case "$os" in
        Linux)
            arch=$(detect_arch "$os")
            libc=$(select_libc)
            printf '%s\n' "$arch-unknown-linux-$libc"
            ;;
        Darwin)
            arch=$(detect_arch "$os")
            printf '%s\n' "$arch-apple-darwin"
            ;;
        *)
            die "unsupported operating system: $os (use install.ps1 on Windows)"
            ;;
    esac
}

download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$2" "$1"
    else
        die 'curl or wget is required'
    fi
}

tmp_dir=
stage_file=

cleanup() {
    if test -n "$stage_file"; then
        rm -f "$stage_file"
    fi
    if test -n "$tmp_dir"; then
        rm -rf "$tmp_dir"
    fi
}

resolve_version() {
    version=${LABELDECK_VERSION:-}
    if test -z "$version"; then
        latest_url=https://github.com/seapagan/labeldeck/releases/latest
        if command -v curl >/dev/null 2>&1; then
            release_url=$(curl -fsSL -o /dev/null -w '%{url_effective}\n' "$latest_url") || \
                die 'could not resolve latest release redirect'
        elif command -v wget >/dev/null 2>&1; then
            headers=$tmp_dir/latest.headers
            wget -q --spider --server-response "$latest_url" 2>"$headers" || \
                die 'could not resolve latest release redirect'
            release_url=$(awk '
                tolower($1) == "location:" { url = $2 }
                END { sub(/\r$/, "", url); print url }
            ' "$headers")
        else
            die 'curl or wget is required'
        fi
        version=$(printf '%s\n' "$release_url" | sed -n \
            's#^https://github\.com/seapagan/labeldeck/releases/tag/\(v[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$#\1#p')
        test -n "$version" || \
            die 'latest release redirect did not resolve to a usable release tag'
    fi
    printf '%s\n' "$version"
}

checksum_tool() {
    os=$(uname -s)
    case "$os" in
        Linux)
            command -v sha256sum >/dev/null 2>&1 || die 'sha256sum is required'
            printf '%s\n' sha256sum
            ;;
        Darwin)
            command -v shasum >/dev/null 2>&1 || die 'shasum is required'
            printf '%s\n' 'shasum -a 256'
            ;;
        *) die "unsupported operating system: $os" ;;
    esac
}

fetch_and_verify_release() {
    version=$1
    target=$2
    explicit_version=$3
    checksum=$(checksum_tool)
    asset="labeldeck-$version-$target.tar.gz"
    base_url="https://github.com/seapagan/labeldeck/releases/download/$version"
    if ! download "$base_url/$asset" "$tmp_dir/$asset"; then
        printf 'error: could not download required release asset: %s\n' "$asset" >&2
        if test "$explicit_version" = 1; then
            printf '%s\n' \
                'If this is an older release, it may predate the current artifact layout.' \
                "See https://github.com/seapagan/labeldeck/releases/tag/$version for the files provided by that release." >&2
        fi
        return 1
    fi
    if ! download "$base_url/$asset.sha256" "$tmp_dir/$asset.sha256"; then
        printf 'error: could not download required checksum asset: %s.sha256\n' "$asset" >&2
        if test "$explicit_version" = 1; then
            printf '%s\n' \
                'If this is an older release, it may predate checksum-backed installer support.' \
                "See https://github.com/seapagan/labeldeck/releases/tag/$version for the files provided by that release." >&2
        fi
        return 1
    fi

    if ! awk -v expected="$asset" '
        NR == 1 && NF == 2 && length($1) == 64 &&
            $1 !~ /[^0-9a-fA-F]/ && $2 == expected { valid = 1 }
        END { exit !(NR == 1 && valid) }
    ' "$tmp_dir/$asset.sha256"; then
        die 'checksum verification failed'
    fi
    if ! (cd "$tmp_dir" && $checksum -c "$asset.sha256"); then
        die 'checksum verification failed'
    fi

    tar -xzf "$tmp_dir/$asset" -C "$tmp_dir"
}

validate_candidate() {
    version=$1
    test -f "$tmp_dir/labeldeck" || die 'release archive is missing labeldeck'
    chmod 755 "$tmp_dir/labeldeck" || die 'release labeldeck is not executable'
    if ! reported_version=$("$tmp_dir/labeldeck" --version); then
        die 'candidate failed --version validation'
    fi
    expected_version="labeldeck ${version#v}"
    test "$reported_version" = "$expected_version" || \
        die "reported version does not match $version: $reported_version"
}

replace_candidate() {
    install_dir=$1
    mkdir -p "$install_dir"
    stage_file=$(mktemp "$install_dir/.labeldeck.XXXXXX") || \
        die 'could not create destination staging file'
    install -m 755 "$tmp_dir/labeldeck" "$stage_file" || \
        die 'could not stage labeldeck in the installation directory'
    mv -f "$stage_file" "$install_dir/labeldeck" || \
        die 'could not atomically replace labeldeck'
    stage_file=
}

main() {
    target=$(detect_target)
    if test -n "${LABELDECK_INSTALL_DIR:-}"; then
        install_dir=$LABELDECK_INSTALL_DIR
    elif test -n "${XDG_BIN_HOME:-}"; then
        install_dir=$XDG_BIN_HOME
    elif test -n "${HOME:-}"; then
        install_dir=$HOME/.local/bin
    else
        die 'no install directory could be determined; set LABELDECK_INSTALL_DIR'
    fi
    explicit_version=0
    if test -n "${LABELDECK_VERSION:-}"; then explicit_version=1; fi

    tmp_dir=$(mktemp -d)
    trap cleanup 0
    trap 'exit 1' 1 2 3 15

    version=$(resolve_version)
    fetch_and_verify_release "$version" "$target" "$explicit_version"
    validate_candidate "$version"

    replace_candidate "$install_dir"

    printf 'Installed labeldeck %s to %s.\n' "$version" "$install_dir"
    case ":${PATH:-}:" in
        *":$install_dir:"*) ;;
        *)
            printf 'warning: %s is not on PATH; add it to PATH to use labeldeck.\n' \
                "$install_dir" >&2
            ;;
    esac
}

main "$@"
