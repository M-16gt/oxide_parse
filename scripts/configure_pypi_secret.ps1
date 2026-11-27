param(
    [string]$Repository = 'M-16gt/oxide_parse',
    [string]$TokenFile = (Join-Path $PSScriptRoot '../.secrets/pypi-token.txt')
)

$ErrorActionPreference = 'Stop'
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw 'Install GitHub CLI (winget install GitHub.cli), then run gh auth login.'
}
if (-not (Test-Path -LiteralPath $TokenFile)) {
    throw "Create $TokenFile and put the PyPI API token on one line."
}
$pypiToken = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $TokenFile)).Trim()
if (-not $pypiToken.StartsWith('pypi-')) {
    throw 'The file must contain a PyPI API token starting with pypi-.'
}
try {
    $pypiToken | & gh secret set PYPI_API_TOKEN --repo $Repository
    if ($LASTEXITCODE -ne 0) { throw 'GitHub secret upload failed.' }
} finally {
    $pypiToken = $null
}
Write-Output "PYPI_API_TOKEN configured for $Repository."
