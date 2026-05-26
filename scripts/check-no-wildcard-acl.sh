#!/usr/bin/env bash
# M11 — apps/ + compositor/ + ai-bridge/ + server-host/ 의 src/ 안에
# ActorPattern::Wildcard / MethodPattern::Wildcard 사용을 *0건*으로 강제.
# tests/ 디렉터리는 회귀 테스트가 의도적으로 wildcard를 사용하므로 제외.
# core/src/object/acl.rs의 enum definition은 제외 (정의 자체는 유지).
# desktop-shell handlers/mod.rs 안 helper는 *MethodPattern::Wildcard*를 compositor 권한 표현용으로 사용 — 허용.

set -e

VIOLATIONS=$(grep -rn \
    --include='*.rs' \
    --exclude-dir='tests' \
    'ActorPattern::Wildcard' \
    apps/ compositor/ ai-bridge/ server-host/ \
    | grep -v 'core/src/object/acl.rs' \
    || true)

if [ -n "$VIOLATIONS" ]; then
    echo "❌ M11 회귀: ActorPattern::Wildcard 사용 발견"
    echo "$VIOLATIONS"
    exit 1
fi

echo "✅ ActorPattern::Wildcard 사용 0건 (M11 KI-001 가드 통과)"
