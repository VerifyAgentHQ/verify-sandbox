$ErrorActionPreference = 'Stop'

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$buildContext = Join-Path $repoRoot 'infrastructure/runner'
$dockerfile = Join-Path $buildContext 'Dockerfile'
$tag = 'verify-agent/runner:development'

& docker build --pull=false --platform linux/amd64 --file $dockerfile --tag $tag $buildContext
if ($LASTEXITCODE -ne 0) {
    throw "runner image build failed with exit code $LASTEXITCODE"
}

& docker image inspect $tag --format '{{.Id}} {{.Os}} {{.Architecture}} {{.Config.User}} {{.Config.WorkingDir}}'
if ($LASTEXITCODE -ne 0) {
    throw "runner image inspection failed with exit code $LASTEXITCODE"
}
