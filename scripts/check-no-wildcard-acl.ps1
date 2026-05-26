# M11 - ActorPattern::Wildcard 사용 grep 가드 (Windows).
# tests/ 디렉터리 제외. core/src/object/acl.rs definition 제외.

$dirs = @("apps", "compositor", "ai-bridge", "server-host")
$matches = $dirs | ForEach-Object {
    Get-ChildItem -Path $_ -Recurse -Filter "*.rs" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch "\\tests\\" } |
        Select-String -Pattern "ActorPattern::Wildcard" -SimpleMatch
}

if ($matches) {
    Write-Host "M11 회귀: ActorPattern::Wildcard 사용 발견" -ForegroundColor Red
    $matches | ForEach-Object { Write-Host "$($_.Path):$($_.LineNumber): $($_.Line.Trim())" }
    exit 1
}

Write-Host "ActorPattern::Wildcard 사용 0건 (M11 KI-001 가드 통과)" -ForegroundColor Green
