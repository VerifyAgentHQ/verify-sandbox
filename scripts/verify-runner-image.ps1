$ErrorActionPreference = 'Stop'

$tag = 'verify-agent/runner:development'
$inspectJson = (& docker image inspect $tag | Out-String | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0 -or $inspectJson.Count -ne 1) {
    throw "expected exactly one local image tagged $tag"
}

$image = $inspectJson[0]
if ($image.Os -ne 'linux') { throw "unexpected image OS: $($image.Os)" }
if ($image.Architecture -ne 'amd64') {
    throw "unexpected image architecture: $($image.Architecture)"
}
if ($image.Config.User -ne '65532:65532') {
    throw "unexpected runtime user: $($image.Config.User)"
}
if ($image.Config.WorkingDir -ne '/workspace') {
    throw "unexpected working directory: $($image.Config.WorkingDir)"
}
if ($null -ne $image.Config.Entrypoint -and $image.Config.Entrypoint.Count -ne 0) {
    throw 'unexpected entrypoint service'
}
if ($null -ne $image.Config.Volumes -and $image.Config.Volumes.Count -ne 0) {
    throw 'unexpected image volume declaration'
}

$envNames = @($image.Config.Env | ForEach-Object { ($_ -split '=', 2)[0] })
if ($envNames | Where-Object { $_ -match '(?i)(password|secret|token|api[_-]?key|credential|docker_host|docker_socket)' }) {
    throw 'credential or Docker socket configuration is present in image environment'
}

$labels = $image.Config.Labels
if (($labels.'org.opencontainers.image.node-version' -ne '24.19.0') -or
    ($labels.'org.opencontainers.image.pnpm-version' -ne '11.21.0') -or
    ($labels.'org.opencontainers.image.rust-version' -ne '1.98.0')) {
    throw 'toolchain labels do not match the pinned versions'
}

$cargoVersion = (& docker run --rm --network none --read-only --user 65532:65532 --workdir /workspace $tag cargo --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $cargoVersion -notmatch '^cargo 1\.98\.0 ') { throw "cargo smoke check failed: $cargoVersion" }
$pnpmVersion = (& docker run --rm --network none --read-only --user 65532:65532 --workdir /workspace $tag pnpm --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $pnpmVersion -ne '11.21.0') { throw "pnpm smoke check failed: $pnpmVersion" }

Write-Output "verified $tag ($($image.Id)); OS=$($image.Os); Architecture=$($image.Architecture); User=$($image.Config.User); WorkDir=$($image.Config.WorkingDir)"
