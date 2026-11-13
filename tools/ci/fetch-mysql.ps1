# Downloads, verifies (pinned size + sha256 from the catalog) and extracts the
# official MySQL no-install ZIP to <Destination>. It contains both the server and
# the client libraries used to build AzerothCore.
#
# The catalog pins the cdn.mysql.com/Downloads/ URL, where Oracle serves the
# *current* GA release; once superseded, the same file moves to
# cdn.mysql.com/archives/, tried as a fallback (same bytes, same sha256).
# dev.mysql.com/get rejects browser-like user agents (HTTP 403), so it is not used.
#
# Usage: pwsh tools/ci/fetch-mysql.ps1 -Manifest catalog/components/mysql-8-4.json -Destination C:\tools\mysql\current
param(
  [Parameter(Mandatory)] [string] $Manifest,
  [Parameter(Mandatory)] [string] $Destination,
  [string] $Platform = 'windows-x86_64'
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (Test-Path (Join-Path $Destination 'bin\mysqld.exe')) {
  Write-Host "MySQL already present at $Destination"
  exit 0
}

$component = Get-Content $Manifest -Raw | ConvertFrom-Json
$build = $component.platforms.$Platform
if (-not $build) { throw "$Manifest has no build for $Platform" }
$version = $component.version
$series = ($version -split '\.')[0..1] -join '.'
$name = "mysql-$version-winx64"
$zip = Join-Path ([IO.Path]::GetTempPath()) "$name.zip"
$urls = @($build.upstream, "https://cdn.mysql.com/archives/mysql-$series/$name.zip") | Select-Object -Unique

$ok = $false
foreach ($url in $urls) {
  for ($attempt = 1; $attempt -le 3 -and -not $ok; $attempt++) {
    try {
      Invoke-WebRequest -Uri $url -OutFile $zip -ConnectionTimeoutSeconds 60 -OperationTimeoutSeconds 300
      $size = (Get-Item $zip).Length
      if ($size -ne [int64]$build.size) { throw "size $size, expected $($build.size)" }
      $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
      if ($hash -ne $build.sha256) { throw "sha256 $hash, expected $($build.sha256)" }
      Write-Host "Downloaded $url (sha256 verified)"
      $ok = $true
    } catch {
      Write-Host "attempt $attempt failed for ${url}: $($_.Exception.Message.Split([Environment]::NewLine)[0])"
      Remove-Item $zip -ErrorAction SilentlyContinue
      if ($_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404) { break }
      Start-Sleep -Seconds 10
    }
  }
  if ($ok) { break }
}
if (-not $ok) { throw "Could not download a verified MySQL $version" }

$parent = Split-Path $Destination -Parent
New-Item -ItemType Directory -Force $parent | Out-Null
Expand-Archive $zip $parent -Force
if (Test-Path $Destination) { Remove-Item -Recurse -Force $Destination }
Rename-Item (Join-Path $parent $name) (Split-Path $Destination -Leaf)
foreach ($required in @($build.executables.PSObject.Properties.Value) + 'lib\mysqlclient.lib') {
  if (-not (Test-Path (Join-Path $Destination $required))) { throw "MySQL archive layout changed: $required missing" }
}
Write-Host "MySQL $version ready at $Destination"
