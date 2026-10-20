#!/usr/bin/env bash
# ==============================================================================
# skTerm One-Command Release & OTA Publishing Script
#
# Usage:
#   ./scripts/release.sh [version] [options]
#
# Examples:
#   ./scripts/release.sh                  # Package & publish current Cargo.toml version
#   ./scripts/release.sh 0.2.0            # Bump version to 0.2.0, test, package, tag & publish
#   ./scripts/release.sh 0.2.0-beta.1 --prerelease
#   ./scripts/release.sh 0.2.0 --skip-checks
#   ./scripts/release.sh 0.2.0 --notes-file docs/release-notes.md
#   ./scripts/release.sh --dry-run
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

TARGET_VERSION=""
SKIP_CHECKS=0
SKIP_PUSH=0
DRY_RUN=0
DRAFT=0
PRERELEASE=0
NOTES_FILE=""
NOTES_TEXT=""

usage() {
    cat << 'EOF'
Usage: ./scripts/release.sh [version] [options]

Arguments:
  [version]              Target version (e.g., 0.2.0 or v0.2.0).
                         Defaults to current [workspace.package] version in Cargo.toml.

Options:
  --skip-checks          Skip cargo fmt, cargo clippy, and cargo test before packaging
  --skip-push            Only build & package locally; do not push tag or publish to GitHub
  --notes-file <path>    Use a custom Markdown file for GitHub Release notes
  --notes <text>         Use custom text for GitHub Release notes
  --draft                Create the GitHub Release as a draft
  --prerelease           Mark the GitHub Release as a pre-release
  --dry-run              Preview all steps without pushing to git or creating GitHub Release
  -h, --help             Show this help message
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --skip-checks)
            SKIP_CHECKS=1
            shift
            ;;
        --skip-push)
            SKIP_PUSH=1
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --draft)
            DRAFT=1
            shift
            ;;
        --prerelease)
            PRERELEASE=1
            shift
            ;;
        --notes-file)
            NOTES_FILE="${2:-}"
            if [ -z "${NOTES_FILE}" ] || [ ! -f "${NOTES_FILE}" ]; then
                echo "Error: --notes-file requires an existing file path." >&2
                exit 1
            fi
            shift 2
            ;;
        --notes)
            NOTES_TEXT="${2:-}"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            echo "Error: Unknown option '$1'" >&2
            usage
            exit 1
            ;;
        *)
            if [ -z "${TARGET_VERSION}" ]; then
                TARGET_VERSION="${1#v}"
                shift
            else
                echo "Error: Unexpected argument '$1'" >&2
                usage
                exit 1
            fi
            ;;
    esac
done

# 1. Read current version from Cargo.toml
CURRENT_VERSION="$(grep -E '^version\s*=\s*"[^"]+"' Cargo.toml | head -n 1 | sed -E 's/.*"([^"]+)".*/\1/')"
if [ -z "${CURRENT_VERSION}" ]; then
    echo "Error: Failed to parse workspace version from Cargo.toml" >&2
    exit 1
fi

if [ -z "${TARGET_VERSION}" ]; then
    VERSION="${CURRENT_VERSION}"
else
    VERSION="${TARGET_VERSION#v}"
fi
TAG="v${VERSION}"
ARCH="$(uname -m)"

echo "=================================================================="
echo "  skTerm Release Automation"
echo "  Current Cargo.toml Version : v${CURRENT_VERSION}"
echo "  Target Release Version     : ${TAG} (${ARCH})"
echo "  Dry Run                    : $([ "${DRY_RUN}" -eq 1 ] && echo "YES" || echo "NO")"
echo "=================================================================="

# 2. Bump version in Cargo.toml & scripts/installer_windows.iss if version changed
if [ "${VERSION}" != "${CURRENT_VERSION}" ]; then
    echo "=== Step 1: Updating workspace version from ${CURRENT_VERSION} -> ${VERSION} ==="
    if [ "${DRY_RUN}" -eq 0 ]; then
        perl -pi -e "s/^version = \"\\Q${CURRENT_VERSION}\\E\"/version = \"${VERSION}\"/" Cargo.toml
        if [ -f "scripts/installer_windows.iss" ]; then
            perl -pi -e "s/#define MyAppVersion \"[^\"]+\"/#define MyAppVersion \"${VERSION}\"/" scripts/installer_windows.iss
        fi
        # Update Cargo.lock workspace version metadata
        cargo check -p skterm-app
    else
        echo "[dry-run] Would update Cargo.toml and scripts/installer_windows.iss to ${VERSION}"
    fi
else
    echo "=== Step 1: Version ${VERSION} already matches Cargo.toml ==="
fi

# 3. Run Quality Checks (fmt, clippy, test)
if [ "${SKIP_CHECKS}" -eq 0 ]; then
    echo "=== Step 2: Running Quality Checks (fmt, clippy, test) ==="
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace -- --test-threads=2
else
    echo "=== Step 2: Skipping Quality Checks (--skip-checks) ==="
fi

# 4. Package macOS Release (.dmg + .app.tar.gz + SHA256SUMS.txt)
echo "=== Step 3: Building & Packaging macOS Release Artifacts ==="
VERSION="${VERSION}" ./scripts/package_macos.sh

DMG_PATH="dist/macos/skTerm-v${VERSION}-macos-${ARCH}.dmg"
OTA_TAR_PATH="dist/macos/skTerm-v${VERSION}-macos-${ARCH}.app.tar.gz"
SHA256_PATH="dist/macos/SHA256SUMS.txt"

for ARTIFACT in "${DMG_PATH}" "${OTA_TAR_PATH}" "${SHA256_PATH}"; do
    if [ ! -f "${ARTIFACT}" ]; then
        echo "Error: Expected artifact not found: ${ARTIFACT}" >&2
        exit 1
    fi
done

echo "=== Verifying Generated SHA256SUMS.txt ==="
cat "${SHA256_PATH}"

if [ "${SKIP_PUSH}" -eq 1 ]; then
    echo "=== Done (--skip-push specified; local artifacts ready in dist/macos/) ==="
    exit 0
fi

# 5. Commit any pending version changes, tag, and push to origin
echo "=== Step 4: Git Commit, Tag (${TAG}), and Push ==="
BRANCH="$(git rev-parse --abbrev-ref HEAD)"

if [ "${DRY_RUN}" -eq 0 ]; then
    if ! git diff --quiet Cargo.toml Cargo.lock scripts/installer_windows.iss 2>/dev/null; then
        git add Cargo.toml Cargo.lock scripts/installer_windows.iss
        git commit -m "chore(release): bump version to ${TAG}"
    fi

    git tag -f "${TAG}"
    git push origin "${BRANCH}"
    git push origin -f "${TAG}"
else
    echo "[dry-run] Would commit version bump (if any), tag ${TAG}, and push ${BRANCH} + ${TAG} to origin"
fi

# 6. Create or Update GitHub Release via gh CLI
echo "=== Step 5: Publishing GitHub Release (${TAG}) ==="
GH_BIN=""
if command -v gh >/dev/null 2>&1; then
    GH_BIN="$(command -v gh)"
elif [ -x "/Users/ericsk/homebrew/bin/gh" ]; then
    GH_BIN="/Users/ericsk/homebrew/bin/gh"
elif [ -x "/opt/homebrew/bin/gh" ]; then
    GH_BIN="/opt/homebrew/bin/gh"
elif [ -x "/usr/local/bin/gh" ]; then
    GH_BIN="/usr/local/bin/gh"
fi

if [ -z "${GH_BIN}" ]; then
    echo "Warning: GitHub CLI (gh) not found. Tag ${TAG} was pushed and GitHub Actions will publish the release."
    exit 0
fi

if [ "${DRY_RUN}" -eq 1 ]; then
    echo "[dry-run] Would publish ${TAG} using ${GH_BIN} with assets:"
    echo "  - ${DMG_PATH}"
    echo "  - ${OTA_TAR_PATH}"
    echo "  - ${SHA256_PATH}"
    exit 0
fi

if "${GH_BIN}" release view "${TAG}" >/dev/null 2>&1; then
    echo "Release ${TAG} already exists on GitHub. Uploading & overwriting updated assets..."
    "${GH_BIN}" release upload "${TAG}" \
        "${DMG_PATH}" \
        "${OTA_TAR_PATH}" \
        "${SHA256_PATH}" \
        --clobber
    if [ -n "${NOTES_FILE}" ]; then
        "${GH_BIN}" release edit "${TAG}" --notes-file "${NOTES_FILE}"
    elif [ -n "${NOTES_TEXT}" ]; then
        "${GH_BIN}" release edit "${TAG}" --notes "${NOTES_TEXT}"
    fi
else
    echo "Creating new GitHub Release ${TAG}..."
    TEMP_NOTES=""
    if [ -z "${NOTES_FILE}" ] && [ -z "${NOTES_TEXT}" ]; then
        TEMP_NOTES="$(mktemp -t skterm-release-notes.XXXXXX)"
        PREV_TAG="$(git tag --sort=-v:refname | grep -v "^${TAG}$" | head -n 1 || true)"
        {
            echo "## skTerm ${TAG}"
            echo ""
            if [ -n "${PREV_TAG}" ]; then
                echo "### What's Changed (since ${PREV_TAG})"
                git log --pretty=format:"- %s (%h)" "${PREV_TAG}..HEAD"
            else
                echo "### Highlights"
                git log -n 20 --pretty=format:"- %s (%h)"
            fi
            echo ""
            echo ""
            echo "### 📦 Installation (macOS Apple Silicon \`${ARCH}\`)"
            echo "1. Download **\`$(basename "${DMG_PATH}")\`** below."
            echo "2. Open the \`.dmg\` and drag **\`skTerm.app\`** into **\`Applications\`**."
            echo "3. If macOS Gatekeeper prompts that the app is not notarized by Apple, run once in Terminal:"
            echo "   \`\`\`bash"
            echo "   xattr -cr /Applications/skTerm.app"
            echo "   \`\`\`"
        } > "${TEMP_NOTES}"
        NOTES_FILE="${TEMP_NOTES}"
    fi

    RELEASE_FLAGS=(--title "skTerm ${TAG}")
    if [ "${DRAFT}" -eq 1 ]; then
        RELEASE_FLAGS+=(--draft)
    fi
    if [ "${PRERELEASE}" -eq 1 ]; then
        RELEASE_FLAGS+=(--prerelease)
    fi
    if [ -n "${NOTES_FILE}" ]; then
        RELEASE_FLAGS+=(--notes-file "${NOTES_FILE}")
    elif [ -n "${NOTES_TEXT}" ]; then
        RELEASE_FLAGS+=(--notes "${NOTES_TEXT}")
    else
        RELEASE_FLAGS+=(--generate-notes)
    fi

    "${GH_BIN}" release create "${TAG}" \
        "${DMG_PATH}" \
        "${OTA_TAR_PATH}" \
        "${SHA256_PATH}" \
        "${RELEASE_FLAGS[@]}"

    if [ -n "${TEMP_NOTES}" ] && [ -f "${TEMP_NOTES}" ]; then
        rm -f "${TEMP_NOTES}"
    fi
fi

RELEASE_URL="$("${GH_BIN}" release view "${TAG}" --json url -q .url 2>/dev/null || echo "https://github.com/ericsk/skTerm/releases/tag/${TAG}")"
echo "=================================================================="
echo "  Successfully released skTerm ${TAG}!"
echo "  Release URL: ${RELEASE_URL}"
echo "=================================================================="
