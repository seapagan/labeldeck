#!/bin/sh
# Deterministic installer tests for install.sh. All network and tool
# interaction is mocked through PATH shims and local release assets; the
# suite never contacts GitHub.

set -eu

root=$(mktemp -d)
trap 'rm -rf "$root"' 0 1 2 3 15

real_bin=$root/real-bin
test_bin=$root/bin
wget_bin=$root/wget-bin
no_download_bin=$root/no-download-bin
no_sha_bin=$root/no-sha-bin
release_dir=$root/release
mkdir -p "$real_bin" "$test_bin" "$wget_bin" "$no_download_bin" \
    "$no_sha_bin" "$release_dir" "$root/archive" "$root/empty" \
    "$root/installer-tmp"

for command in awk cat chmod cp find grep gzip head mkdir rm sed wc; do
    path=$(command -v "$command")
    ln -s "$path" "$real_bin/$command"
done
for directory in "$test_bin" "$wget_bin" "$no_download_bin" "$no_sha_bin"; do
    for path in "$real_bin"/*; do
        ln -s "$path" "$directory/${path##*/}"
    done
done

REAL_INSTALL=$(command -v install)
REAL_MV=$(command -v mv)
REAL_SHA256SUM=$(command -v sha256sum)
REAL_TAR=$(command -v tar)
export REAL_INSTALL REAL_MV REAL_SHA256SUM REAL_TAR

cat >"$test_bin/uname" <<'EOF'
#!/bin/sh
case "$1" in
    -s) printf '%s\n' "${TEST_OS:-Linux}" ;;
    -m) printf '%s\n' "${TEST_ARCH:-x86_64}" ;;
    *) exit 2 ;;
esac
EOF

cat >"$test_bin/getconf" <<'EOF'
#!/bin/sh
case "${TEST_GETCONF_KIND:-glibc}" in
    glibc) printf 'glibc %s\n' "$TEST_GLIBC_VERSION" ;;
    malformed) printf 'glibc unknown\n' ;;
    unavailable) exit 1 ;;
esac
EOF

cat >"$test_bin/ldd" <<'EOF'
#!/bin/sh
case "${TEST_LDD_KIND:-glibc}" in
    glibc) printf 'ldd (GNU libc) %s\n' "$TEST_GLIBC_VERSION" ;;
    musl) printf 'musl libc (%s)\n' "${TEST_ARCH:-x86_64}" >&2 ;;
    malformed) printf 'ldd mystery libc\n' ;;
    unavailable) exit 1 ;;
esac
EOF

cat >"$test_bin/mktemp" <<'EOF'
#!/bin/sh
case "$1" in
    -d)
        count=$(cat "$TEST_MKTEMP_COUNTER")
        count=$((count + 1))
        printf '%s\n' "$count" >"$TEST_MKTEMP_COUNTER"
        directory="$TEST_TMP_PARENT/run-$count"
        mkdir "$directory"
        printf '%s\n' "$directory" >"$TEST_MKTEMP_LAST"
        printf '%s\n' "$directory"
        ;;
    *)
        test "$TEST_STAGE_FAIL" != 1 || exit 1
        count=$(cat "$TEST_STAGE_COUNTER")
        count=$((count + 1))
        printf '%s\n' "$count" >"$TEST_STAGE_COUNTER"
        path=${1%XXXXXX}$count
        : >"$path"
        printf '%s\n' "$path" >"$TEST_STAGE_LAST"
        printf '%s\n' "$path"
        ;;
esac
EOF

cat >"$test_bin/curl" <<'EOF'
#!/bin/sh
url=
destination=
while test "$#" -gt 0; do
    case "$1" in
        -o) destination=$2; shift 2 ;;
        -w) shift 2 ;;
        -*) shift ;;
        *) url=$1; shift ;;
    esac
done
printf '%s\n' "$url" >>"$TEST_DOWNLOAD_LOG"
printf 'curl\n' >>"$TEST_DOWNLOADER_LOG"
case "$url" in
    */releases/latest) printf '%s\n' "$TEST_LATEST_URL" ;;
    */releases/download/*)
        name=${url##*/}
        cp "$TEST_RELEASE_DIR/$name" "$destination"
        ;;
    *) exit 1 ;;
esac
EOF

cat >"$test_bin/wget" <<'EOF'
#!/bin/sh
url=
destination=
while test "$#" -gt 0; do
    case "$1" in
        -qO) destination=$2; shift 2 ;;
        -*) shift ;;
        *) url=$1; shift ;;
    esac
done
printf '%s\n' "$url" >>"$TEST_DOWNLOAD_LOG"
printf 'wget\n' >>"$TEST_DOWNLOADER_LOG"
case "$url" in
    */releases/latest) printf '  Location: %s\n' "$TEST_LATEST_URL" >&2 ;;
    */releases/download/*)
        name=${url##*/}
        cp "$TEST_RELEASE_DIR/$name" "$destination"
        ;;
    *) exit 1 ;;
esac
EOF

cat >"$test_bin/sha256sum" <<'EOF'
#!/bin/sh
printf 'sha256sum\n' >>"$TEST_EVENT_LOG"
exec "$REAL_SHA256SUM" "$@"
EOF

# shasum shim: lets Darwin-mode runs execute on any Unix by translating
# to the local sha256sum (identical "HASH  NAME" wire format).
cat >"$test_bin/shasum" <<'EOF'
#!/bin/sh
printf 'shasum\n' >>"$TEST_EVENT_LOG"
args=
while test "$#" -gt 0; do
    case "$1" in
        -a) shift 2 ;;
        *) args="$args $1"; shift ;;
    esac
done
exec "$REAL_SHA256SUM" $args
EOF

cat >"$test_bin/tar" <<'EOF'
#!/bin/sh
printf 'tar\n' >>"$TEST_EVENT_LOG"
exec "$REAL_TAR" "$@"
EOF

cat >"$test_bin/install" <<'EOF'
#!/bin/sh
printf 'install\n' >>"$TEST_EVENT_LOG"
if test "$TEST_INSTALL_SIGNAL" = 1; then
    kill -TERM "$PPID"
    exit 1
fi
test "$TEST_INSTALL_FAIL" != 1 || exit 1
exec "$REAL_INSTALL" "$@"
EOF

cat >"$test_bin/mv" <<'EOF'
#!/bin/sh
printf 'mv\n' >>"$TEST_EVENT_LOG"
test "$TEST_MOVE_FAIL" != 1 || exit 1
exec "$REAL_MV" "$@"
EOF

chmod +x \
    "$test_bin/uname" \
    "$test_bin/getconf" \
    "$test_bin/ldd" \
    "$test_bin/mktemp" \
    "$test_bin/curl" \
    "$test_bin/wget" \
    "$test_bin/sha256sum" \
    "$test_bin/shasum" \
    "$test_bin/tar" \
    "$test_bin/install" \
    "$test_bin/mv"
for name in uname getconf ldd mktemp tar install mv sha256sum shasum; do
    for directory in "$wget_bin" "$no_download_bin" "$no_sha_bin"; do
        if test "$directory:$name" = "$no_sha_bin:sha256sum"; then
            continue
        fi
        rm -f "$directory/$name"
        cp "$test_bin/$name" "$directory/$name"
    done
done
cp "$test_bin/wget" "$wget_bin/wget"
cp "$test_bin/curl" "$test_bin/wget" "$no_sha_bin/"

cat >"$root/archive/labeldeck" <<'EOF'
#!/bin/sh
printf 'candidate\n' >>"$TEST_EVENT_LOG"
test "${TEST_CANDIDATE_EXIT:-0}" = 0 || exit "$TEST_CANDIDATE_EXIT"
printf 'labeldeck %s\n' "${TEST_CANDIDATE_VERSION:-99.88.77}"
EOF
chmod +x "$root/archive/labeldeck"
printf 'release readme\n' >"$root/archive/README.md"
printf 'release licence\n' >"$root/archive/LICENSE-MIT"

# Synthetic versions used only by this test harness.
# They deliberately do not correspond to real labeldeck releases.
TEST_RELEASE_VERSION=99.88.77
TEST_RELEASE_TAG=v$TEST_RELEASE_VERSION
TEST_OLD_VERSION=98.77.66
TEST_OLD_TAG=v$TEST_OLD_VERSION

targets='x86_64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-gnu aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin'
for target in $targets; do
    "$REAL_TAR" -czf "$release_dir/labeldeck-$TEST_RELEASE_TAG-$target.tar.gz" \
        -C "$root/archive" labeldeck README.md LICENSE-MIT
    (cd "$release_dir" && "$REAL_SHA256SUM" \
        "labeldeck-$TEST_RELEASE_TAG-$target.tar.gz" \
        >"labeldeck-$TEST_RELEASE_TAG-$target.tar.gz.sha256")
done
"$REAL_TAR" -czf "$release_dir/missing.tar.gz" -C "$root/empty" .
TEST_LATEST_URL=https://github.com/seapagan/labeldeck/releases/tag/$TEST_RELEASE_TAG
TEST_RELEASE_DIR=$release_dir
TEST_DOWNLOAD_LOG=$root/downloads.log
TEST_DOWNLOADER_LOG=$root/downloaders.log
TEST_EVENT_LOG=$root/events.log
TEST_TMP_PARENT=$root/installer-tmp
TEST_MKTEMP_COUNTER=$root/mktemp-counter
TEST_MKTEMP_LAST=$root/mktemp-last
TEST_STAGE_COUNTER=$root/stage-counter
TEST_STAGE_LAST=$root/stage-last
export TEST_LATEST_URL TEST_RELEASE_DIR TEST_DOWNLOAD_LOG TEST_DOWNLOADER_LOG
export TEST_EVENT_LOG TEST_TMP_PARENT TEST_MKTEMP_COUNTER TEST_MKTEMP_LAST
export TEST_STAGE_COUNTER TEST_STAGE_LAST

printf '0\n' >"$TEST_MKTEMP_COUNTER"
printf '0\n' >"$TEST_STAGE_COUNTER"
passes=0

fail() {
    printf 'FAILED: %s\n' "$1" >&2
    exit 1
}

pass() { passes=$((passes + 1)); }

LABELDECK_INSTALL_DIR=$root/install
export LABELDECK_INSTALL_DIR

reset_env() {
    rm -rf "${LABELDECK_INSTALL_DIR:-}"
    rm -f "$TEST_DOWNLOAD_LOG" "$TEST_DOWNLOADER_LOG" "$TEST_EVENT_LOG"
    printf '0\n' >"$TEST_MKTEMP_COUNTER"
    printf '0\n' >"$TEST_STAGE_COUNTER"
    unset LABELDECK_VERSION LABELDECK_LIBC XDG_BIN_HOME \
        TEST_OS TEST_ARCH TEST_GLIBC_VERSION TEST_GETCONF_KIND TEST_LDD_KIND \
        TEST_CANDIDATE_EXIT TEST_CANDIDATE_VERSION TEST_STAGE_FAIL \
        TEST_INSTALL_FAIL TEST_MOVE_FAIL TEST_INSTALL_SIGNAL TEST_BIN
    TEST_GLIBC_VERSION=2.39
    TEST_LATEST_URL=https://github.com/seapagan/labeldeck/releases/tag/$TEST_RELEASE_TAG
    export TEST_GLIBC_VERSION TEST_LATEST_URL
    LABELDECK_INSTALL_DIR=$root/install
    export LABELDECK_INSTALL_DIR
}

run_install() {
    env PATH="${TEST_BIN:-$test_bin}" /bin/sh ./install.sh >"$root/output" 2>&1
}

assert_fails() {
    if run_install; then fail "$1"; fi
}

assert_output() { grep -Fq "$1" "$root/output" || fail "missing output: $1"; }

assert_no_output() {
    if grep -Fq "$1" "$root/output"; then fail "unexpected output: $1"; fi
}

assert_downloaded() {
    grep -Fxq "https://github.com/seapagan/labeldeck/releases/download/$TEST_RELEASE_TAG/$1" \
        "$TEST_DOWNLOAD_LOG" || fail "asset was not downloaded: $1"
}

assert_target() {
    assert_target_named "$1"
    assert_downloaded "labeldeck-$TEST_RELEASE_TAG-$1.tar.gz"
}

assert_target_named() {
    test -x "$LABELDECK_INSTALL_DIR/labeldeck" || fail "labeldeck was not installed"
}

assert_old_binary() {
    test "$(cat "$LABELDECK_INSTALL_DIR/labeldeck")" = 'old labeldeck' || \
        fail 'existing labeldeck changed after failure'
}

assert_no_binary() {
    test ! -e "$LABELDECK_INSTALL_DIR/labeldeck" || \
        fail 'failed installation created a final binary'
}

assert_tmp_cleaned() {
    test -z "$(find "$TEST_TMP_PARENT" -mindepth 1 -print)" || \
        fail 'temporary files were not cleaned up'
}

for case_data in 'x86_64:x86_64-unknown-linux-gnu' \
    'amd64:x86_64-unknown-linux-gnu' \
    'aarch64:aarch64-unknown-linux-gnu' \
    'arm64:aarch64-unknown-linux-gnu'; do
    arch=${case_data%%:*}
    target=${case_data#*:}
    reset_env
    TEST_ARCH=$arch
    TEST_GLIBC_VERSION=2.39
    export TEST_ARCH TEST_GLIBC_VERSION
    run_install || fail "$arch target failed"
    assert_target "$target"
    pass
done

# Darwin: target selection without any libc detection.
for case_data in 'x86_64:x86_64-apple-darwin' 'arm64:aarch64-apple-darwin'; do
    arch=${case_data%%:*}
    target=${case_data#*:}
    reset_env
    TEST_OS=Darwin
    TEST_ARCH=$arch
    TEST_GETCONF_KIND=unavailable
    TEST_LDD_KIND=unavailable
    export TEST_OS TEST_ARCH TEST_GETCONF_KIND TEST_LDD_KIND
    run_install || fail "$arch Darwin target failed"
    assert_target "$target"
    grep -Fxq shasum "$TEST_EVENT_LOG" || fail 'Darwin must use shasum'
    if grep -Eq '^(getconf|ldd)$' "$TEST_EVENT_LOG"; then
        fail 'Darwin must not detect libc'
    fi
    pass
done

reset_env
TEST_OS=FreeBSD
export TEST_OS
assert_fails 'unsupported operating system succeeded'
assert_output 'unsupported operating system: FreeBSD'
assert_no_binary
pass

reset_env
TEST_ARCH=ppc64le
export TEST_ARCH
assert_fails 'unsupported architecture succeeded'
assert_output 'unsupported platform: Linux/ppc64le'
pass

for case_data in '2.39:gnu' '2.28:gnu' '2.27:musl' '1.17:musl'; do
    version=${case_data%%:*}
    expected_libc=${case_data#*:}
    reset_env
    TEST_GLIBC_VERSION=$version
    export TEST_GLIBC_VERSION
    run_install || fail "glibc $version selection failed"
    assert_target "x86_64-unknown-linux-$expected_libc"
    if test "$expected_libc" = musl; then
        assert_output 'older than labeldeck minimum 2.28'
    fi
    pass
done

reset_env
TEST_GETCONF_KIND=unavailable
TEST_LDD_KIND=musl
export TEST_GETCONF_KIND TEST_LDD_KIND
run_install || fail 'musl detection failed'
assert_target x86_64-unknown-linux-musl
pass

reset_env
TEST_GETCONF_KIND=unavailable
TEST_LDD_KIND=glibc
TEST_GLIBC_VERSION=2.39
export TEST_GETCONF_KIND TEST_LDD_KIND TEST_GLIBC_VERSION
run_install || fail 'ldd glibc fallback failed'
assert_target x86_64-unknown-linux-gnu
pass

reset_env
TEST_GETCONF_KIND=unavailable
TEST_LDD_KIND=unavailable
export TEST_GETCONF_KIND TEST_LDD_KIND
assert_fails 'unknown libc succeeded'
assert_output 'unable to detect libc'
assert_output 'LABELDECK_LIBC=gnu or LABELDECK_LIBC=musl'
pass

reset_env
TEST_GETCONF_KIND=malformed
TEST_LDD_KIND=malformed
export TEST_GETCONF_KIND TEST_LDD_KIND
assert_fails 'malformed libc data succeeded'
assert_output 'unable to detect libc'
pass

for libc in gnu musl; do
    reset_env
    LABELDECK_LIBC=$libc
    export LABELDECK_LIBC
    run_install || fail "libc override $libc failed"
    assert_target "x86_64-unknown-linux-$libc"
    pass
done

reset_env
TEST_ARCH=aarch64
LABELDECK_LIBC=musl
export TEST_ARCH LABELDECK_LIBC
run_install || fail 'aarch64 musl target failed'
assert_target aarch64-unknown-linux-musl
pass

reset_env
LABELDECK_LIBC=other
export LABELDECK_LIBC
assert_fails 'invalid libc override succeeded'
assert_output 'LABELDECK_LIBC must be gnu or musl'
pass

reset_env
LABELDECK_LIBC=gnu
TEST_GLIBC_VERSION=2.17
TEST_CANDIDATE_EXIT=127
mkdir -p "$LABELDECK_INSTALL_DIR"
printf 'old labeldeck\n' >"$LABELDECK_INSTALL_DIR/labeldeck"
export LABELDECK_LIBC TEST_GLIBC_VERSION TEST_CANDIDATE_EXIT
assert_fails 'incompatible forced GNU candidate succeeded'
assert_old_binary
pass

reset_env
run_install || fail 'target URLs failed'
assert_target x86_64-unknown-linux-gnu
downloads=$(grep -Fc '/releases/download/' "$TEST_DOWNLOAD_LOG" || true)
test "$downloads" -eq 2 || fail 'wrong download count'
pass

reset_env
run_install || fail 'valid checksum failed'
grep -Fxq sha256sum "$TEST_EVENT_LOG" || fail 'sha256sum did not run'
pass

for mode in bad malformed wrong-name external-file; do
    reset_env
    archive=labeldeck-$TEST_RELEASE_TAG-x86_64-unknown-linux-gnu.tar.gz
    sidecar=$release_dir/$archive.sha256
    backup=$root/sidecar-backup
    cp "$sidecar" "$backup"
    case $mode in
        bad) printf '%064d  %s\n' 0 "$archive" >"$sidecar" ;;
        malformed) printf 'not-a-checksum  %s\n' "$archive" >"$sidecar" ;;
        wrong-name) printf '%s  other-file\n' "$(cut -d' ' -f1 "$backup")" >"$sidecar" ;;
        external-file) printf '%s  /etc/passwd\n' "$(cut -d' ' -f1 "$backup")" >"$sidecar" ;;
    esac
    assert_fails "bad checksum ($mode) succeeded"
    assert_output 'checksum verification failed'
    assert_no_binary
    cp "$backup" "$sidecar"
    pass
done

reset_env
TEST_BIN=$no_sha_bin
export TEST_BIN
assert_fails 'missing sha256sum succeeded'
assert_output 'sha256sum is required'
assert_no_binary
pass

reset_env
TEST_CANDIDATE_EXIT=1
mkdir -p "$LABELDECK_INSTALL_DIR"
printf 'old labeldeck\n' >"$LABELDECK_INSTALL_DIR/labeldeck"
export TEST_CANDIDATE_EXIT
assert_fails 'candidate non-zero exit succeeded'
assert_old_binary
pass

reset_env
TEST_CANDIDATE_VERSION=9.9.9
export TEST_CANDIDATE_VERSION
assert_fails 'wrong candidate version succeeded'
assert_output "reported version does not match $TEST_RELEASE_TAG"
assert_no_binary
pass

reset_env
bad_archive=$root/bad-archive
mkdir "$bad_archive"
printf '#!/missing/interpreter\n' >"$bad_archive/labeldeck"
chmod +x "$bad_archive/labeldeck"
cp "$root/archive/README.md" "$root/archive/LICENSE-MIT" "$bad_archive/"
archive=labeldeck-$TEST_RELEASE_TAG-x86_64-unknown-linux-gnu.tar.gz
"$REAL_TAR" -czf "$release_dir/$archive" -C "$bad_archive" labeldeck README.md LICENSE-MIT
(cd "$release_dir" && "$REAL_SHA256SUM" "$archive" >"$archive.sha256")
assert_fails 'unexecutable candidate succeeded'
assert_output 'candidate failed --version validation'
assert_no_binary
"$REAL_TAR" -czf "$release_dir/$archive" -C "$root/archive" labeldeck README.md LICENSE-MIT
(cd "$release_dir" && "$REAL_SHA256SUM" "$archive" >"$archive.sha256")
pass

reset_env
LABELDECK_INSTALL_DIR=$root/success-bin
export LABELDECK_INSTALL_DIR
run_install || fail 'successful replacement failed'
test -x "$LABELDECK_INSTALL_DIR/labeldeck" || fail 'installed binary is not executable'
test "$("$LABELDECK_INSTALL_DIR/labeldeck" --version)" = "labeldeck $TEST_RELEASE_VERSION" || \
    fail 'installed candidate is wrong'
test ! -e "$LABELDECK_INSTALL_DIR/README.md" || fail 'README was installed'
test ! -e "$LABELDECK_INSTALL_DIR/LICENSE-MIT" || fail 'licence was installed'
pass

for failure in stage install move; do
    reset_env
    mkdir -p "$LABELDECK_INSTALL_DIR"
    printf 'old labeldeck\n' >"$LABELDECK_INSTALL_DIR/labeldeck"
    case $failure in
        stage) TEST_STAGE_FAIL=1 ;;
        install) TEST_INSTALL_FAIL=1 ;;
        move) TEST_MOVE_FAIL=1 ;;
    esac
    export TEST_STAGE_FAIL TEST_INSTALL_FAIL TEST_MOVE_FAIL
    assert_fails "$failure failure succeeded"
    assert_old_binary
    test -z "$(find "$LABELDECK_INSTALL_DIR" -name '.labeldeck.*' -print)" || \
        fail "$failure failure left a destination staging file"
    pass
done

reset_env
mkdir -p "$LABELDECK_INSTALL_DIR"
printf 'old labeldeck\n' >"$LABELDECK_INSTALL_DIR/labeldeck"
TEST_INSTALL_SIGNAL=1
export TEST_INSTALL_SIGNAL
assert_fails 'interrupted install succeeded'
assert_old_binary
test -z "$(find "$LABELDECK_INSTALL_DIR" -name '.labeldeck.*' -print)" || \
    fail 'interrupt left a destination staging file'
pass

reset_env
mkdir -p "$LABELDECK_INSTALL_DIR"
printf 'old labeldeck\n' >"$LABELDECK_INSTALL_DIR/labeldeck"
run_install || fail 'replacement of existing binary failed'
test "$("$LABELDECK_INSTALL_DIR/labeldeck" --version)" = "labeldeck $TEST_RELEASE_VERSION" || \
    fail 'existing binary was not replaced'
pass

reset_env
LABELDECK_VERSION=
export LABELDECK_VERSION
run_install || fail 'latest-release lookup failed'
grep -Fxq 'https://github.com/seapagan/labeldeck/releases/latest' "$TEST_DOWNLOAD_LOG" || \
    fail 'GitHub latest-release redirect was not used'
if grep -Fq 'api.github.com' "$TEST_DOWNLOAD_LOG"; then
    fail 'latest lookup used api.github.com'
fi
assert_target x86_64-unknown-linux-gnu
pass

reset_env
LABELDECK_VERSION=$TEST_RELEASE_TAG
export LABELDECK_VERSION
run_install || fail 'explicit version failed'
if grep -Fq '/releases/latest' "$TEST_DOWNLOAD_LOG"; then
    fail 'explicit version queried latest release'
fi
pass

reset_env
LABELDECK_VERSION=
TEST_LATEST_URL=https://github.com/seapagan/labeldeck/releases/tag/not-a-version
export LABELDECK_VERSION TEST_LATEST_URL
assert_fails 'malformed latest-release redirect succeeded'
assert_output 'latest release redirect did not resolve to a usable release tag'
if grep -Fq '/releases/download/' "$TEST_DOWNLOAD_LOG"; then
    fail 'malformed latest redirect attempted an asset download'
fi
pass

for install_state in existing fresh; do
    # Old release ships only the checksum sidecar: the archive itself is
    # missing (pre-layout release).
    old_archive=labeldeck-$TEST_OLD_TAG-x86_64-unknown-linux-gnu.tar.gz
    printf '%s  %s\n' "$(printf 'x%.0s' $(seq 64))" "$old_archive" \
        >"$release_dir/$old_archive.sha256"
    reset_env
    LABELDECK_VERSION=$TEST_OLD_TAG
    export LABELDECK_VERSION
    if test "$install_state" = existing; then
        mkdir -p "$LABELDECK_INSTALL_DIR"
        printf 'old labeldeck\n' >"$LABELDECK_INSTALL_DIR/labeldeck"
    fi
    assert_fails 'missing archive asset succeeded'
    assert_output 'could not download required release asset'
    if test "$install_state" = existing; then
        assert_old_binary
    else
        assert_no_binary
    fi
    assert_output 'may predate the current artifact layout'
    rm "$release_dir/$old_archive.sha256"
    pass
done

for install_state in existing fresh; do
    # Old release ships the archive but predates checksum sidecars.
    old_archive=labeldeck-$TEST_OLD_TAG-x86_64-unknown-linux-gnu.tar.gz
    cp "$release_dir/labeldeck-$TEST_RELEASE_TAG-x86_64-unknown-linux-gnu.tar.gz" \
        "$release_dir/$old_archive"
    reset_env
    LABELDECK_VERSION=$TEST_OLD_TAG
    export LABELDECK_VERSION
    if test "$install_state" = existing; then
        mkdir -p "$LABELDECK_INSTALL_DIR"
        printf 'old labeldeck\n' >"$LABELDECK_INSTALL_DIR/labeldeck"
    fi
    assert_fails 'missing checksum asset succeeded'
    assert_output 'could not download required checksum asset'
    if test "$install_state" = existing; then
        assert_old_binary
    else
        assert_no_binary
    fi
    assert_output 'may predate checksum-backed installer support'
    rm "$release_dir/$old_archive"
    pass
done

current_archive=labeldeck-$TEST_RELEASE_TAG-x86_64-unknown-linux-gnu.tar.gz
mv "$release_dir/$current_archive" "$root/current-archive"
for install_state in existing fresh; do
    reset_env
    if test "$install_state" = existing; then
        mkdir -p "$LABELDECK_INSTALL_DIR"
        printf 'old labeldeck\n' >"$LABELDECK_INSTALL_DIR/labeldeck"
    fi
    assert_fails 'missing current archive succeeded'
    assert_output 'could not download required release asset'
    if test "$install_state" = existing; then
        assert_old_binary
    else
        assert_no_binary
    fi
    pass
done
mv "$root/current-archive" "$release_dir/$current_archive"

mv "$release_dir/$current_archive.sha256" "$root/current-checksum"
for install_state in existing fresh; do
    reset_env
    if test "$install_state" = existing; then
        mkdir -p "$LABELDECK_INSTALL_DIR"
        printf 'old labeldeck\n' >"$LABELDECK_INSTALL_DIR/labeldeck"
    fi
    assert_fails 'missing current checksum succeeded'
    assert_output 'could not download required checksum asset'
    if test "$install_state" = existing; then
        assert_old_binary
    else
        assert_no_binary
    fi
    pass
done
mv "$root/current-checksum" "$release_dir/$current_archive.sha256"

reset_env
LABELDECK_VERSION=
export LABELDECK_VERSION
run_install || fail 'curl latest-resolution path failed'
grep -Fxq curl "$TEST_DOWNLOADER_LOG" || fail 'curl was not preferred'
pass

reset_env
TEST_BIN=$wget_bin
export TEST_BIN
LABELDECK_VERSION=
export LABELDECK_VERSION
run_install || fail 'wget latest-resolution fallback failed'
grep -Fxq wget "$TEST_DOWNLOADER_LOG" || fail 'wget did not run'
pass

reset_env
TEST_BIN=$no_download_bin
export TEST_BIN
assert_fails 'missing downloader succeeded'
assert_output 'curl or wget is required'
assert_tmp_cleaned
pass

reset_env
LABELDECK_INSTALL_DIR=$root/'explicit bin'
XDG_BIN_HOME=$root/ignored-xdg
HOME=$root/ignored-home
export LABELDECK_INSTALL_DIR XDG_BIN_HOME HOME
run_install || fail 'explicit install directory failed'
test -x "$LABELDECK_INSTALL_DIR/labeldeck" || fail 'explicit install directory was ignored'
test ! -e "$XDG_BIN_HOME/labeldeck" || fail 'XDG overrode explicit directory'
pass

reset_env
XDG_BIN_HOME=$root/'xdg bin'
LABELDECK_INSTALL_DIR=
export XDG_BIN_HOME LABELDECK_INSTALL_DIR
run_install || fail 'XDG install directory failed'
test -x "$XDG_BIN_HOME/labeldeck" || fail 'XDG directory was ignored'
pass

reset_env
HOME=$root/'default home'
LABELDECK_INSTALL_DIR=
export HOME LABELDECK_INSTALL_DIR
run_install || fail 'default install directory failed'
test -x "$HOME/.local/bin/labeldeck" || fail 'default directory was ignored'
pass

reset_env
unset LABELDECK_INSTALL_DIR XDG_BIN_HOME HOME
assert_fails 'missing install directory inputs succeeded'
assert_output 'no install directory could be determined; set LABELDECK_INSTALL_DIR'
test ! -s "$TEST_DOWNLOAD_LOG" || \
    fail 'missing install directory inputs attempted a release download'
pass

reset_env
archive=labeldeck-$TEST_RELEASE_TAG-x86_64-unknown-linux-gnu.tar.gz
cp "$release_dir/missing.tar.gz" "$release_dir/$archive"
(cd "$release_dir" && "$REAL_SHA256SUM" "$archive" >"$archive.sha256")
assert_fails 'archive without labeldeck succeeded'
assert_output 'release archive is missing labeldeck'
assert_no_binary
"$REAL_TAR" -czf "$release_dir/$archive" -C "$root/archive" labeldeck README.md LICENSE-MIT
(cd "$release_dir" && "$REAL_SHA256SUM" "$archive" >"$archive.sha256")
pass

reset_env
LABELDECK_INSTALL_DIR=$root/not-on-path
export LABELDECK_INSTALL_DIR
run_install || fail 'PATH warning setup failed'
assert_output 'add it to PATH'
pass

reset_env
LABELDECK_INSTALL_DIR=$root/'on path'
TEST_BIN="$test_bin:$LABELDECK_INSTALL_DIR"
export LABELDECK_INSTALL_DIR TEST_BIN
run_install || fail 'exact PATH setup failed'
if grep -Fq 'add it to PATH' "$root/output"; then
    fail 'exact PATH component produced a warning'
fi
assert_tmp_cleaned
pass

printf 'Unix installer tests passed: %s cases.\n' "$passes"
