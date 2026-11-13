# Downloads and extracts the official MySQL no-install ZIP to <Destination>
# (contains both the server and the client libraries used to build AzerothCore).
#
# Oracle serves the *current* GA release under cdn.mysql.com/Downloads/ and older
# releases under cdn.mysql.com/archives/. dev.mysql.com/get rejects browser-like
# user agents (HTTP 403), so it is not used.
#
# Usage: pwsh tools/ci/fetch-mysql.ps1 -Version 8.4.13 -Destination C:\tools\mysql\current
param(
  [Parameter(Mandatory)] [string] $Version,
  [Parameter(Mandatory)] [string] $Destination
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (Test-Path (Join-Path $Destination 'bin\mysqld.exe')) {
  Write-Host "MySQL already present at $Destination"
  exit 0
}

$series = ($Version -split '\.')[0..1] -join '.'
$name = "mysql-$Version-winx64"
$zip = Join-Path ([IO.Path]::GetTempPath()) "$name.zip"
$urls = @(
  "https://cdn.mysql.com/Downloads/MySQL-$series/$name.zip",
  "https://cdn.mysql.com/archives/mysql-$series/$name.zip"
)

$ok = $false
foreach ($url in $urls) {
  for ($attempt = 1; $attempt -le 3 -and -not $ok; $attempt++) {
    try {
      Invoke-WebRequest -Uri $url -OutFile $zip -ConnectionTimeoutSeconds 60 -OperationTimeoutSeconds 120
      $mb = (Get-Item $zip).Length / 1MB
      if ($mb -lt 50) { throw "got $([int]$mb) MB (probably an error page)" }
      Write-Host ("Downloaded {0} ({1:N1} MB)" -f $url, $mb)
      $ok = $true
    } catch {
      Write-Host "attempt $attempt failed for ${url}: $($_.Exception.Message.Split([Environment]::NewLine)[0])"
      if ($_.Exception.Response -and [int]$_.Exception.Response.StatusCode -eq 404) { break }
      Start-Sleep -Seconds 10
    }
  }
  if ($ok) { break }
}
if (-not $ok) { throw "Could not download MySQL $Version" }

$parent = Split-Path $Destination -Parent
New-Item -ItemType Directory -Force $parent | Out-Null
Expand-Archive $zip $parent -Force
if (Test-Path $Destination) { Remove-Item -Recurse -Force $Destination }
Rename-Item (Join-Path $parent $name) (Split-Path $Destination -Leaf)
foreach ($required in 'bin\mysqld.exe', 'bin\mysql.exe', 'lib\mysqlclient.lib') {
  if (-not (Test-Path (Join-Path $Destination $required))) { throw "MySQL archive layout changed: $required missing" }
}
Write-Host "MySQL $Version ready at $Destination"
