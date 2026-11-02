& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $release = 'https://github.com/Qznad/unbake/releases/latest/download'
    $folder = if ($env:UNBAKE_DIR) { $env:UNBAKE_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\unbake' }
    $exe = Join-Path $folder 'unbake.exe'

    # downloads the latest unbake.exe and refuses it unless it matches its published checksum
    function Get-Unbake($target) {
        if ($env:UNBAKE_FROM) {
            Copy-Item $env:UNBAKE_FROM $target
            return
        }
        Invoke-WebRequest "$release/unbake.exe" -OutFile $target -UseBasicParsing
        $sums = Invoke-WebRequest "$release/unbake.exe.sha256" -UseBasicParsing
        $text = if ($sums.Content -is [byte[]]) { [Text.Encoding]::ASCII.GetString($sums.Content) } else { $sums.Content }
        $expected = ($text.Trim() -split '\s+')[0].ToLower()
        $actual = (Get-FileHash $target -Algorithm SHA256).Hash.ToLower()
        if ($actual -ne $expected) {
            Remove-Item $target
            throw "The download didn't match its checksum, so nothing was installed. Try again in a minute."
        }
    }

    # puts the new exe in place, moving a running copy aside since windows lets you rename it but not replace it
    function Install-Unbake($download) {
        Get-ChildItem $folder -Filter 'unbake.old-*.exe' | ForEach-Object {
            Remove-Item $_.FullName -ErrorAction SilentlyContinue
        }
        if (Test-Path $exe) {
            Move-Item $exe (Join-Path $folder "unbake.old-$(Get-Date -Format yyyyMMddHHmmss).exe")
        }
        Move-Item $download $exe
    }

    # adds the folder to the user's PATH for this window and every new one, keeping the PATH's own %variables%
    function Add-ToPath {
        $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
        $saved = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $parts = @($saved -split ';' | Where-Object { $_ })
        if ($parts -notcontains $folder) {
            $key.SetValue('Path', (($parts + $folder) -join ';'), [Microsoft.Win32.RegistryValueKind]::ExpandString)
            [Environment]::SetEnvironmentVariable('UNBAKE_INSTALLING', '1', 'User')
            [Environment]::SetEnvironmentVariable('UNBAKE_INSTALLING', $null, 'User')
        }
        $key.Close()
        if (($env:Path -split ';') -notcontains $folder) {
            $env:Path = "$env:Path;$folder"
        }
    }

    Write-Host 'Installing unbake...'
    New-Item -ItemType Directory -Force $folder | Out-Null
    $download = Join-Path $folder 'unbake.download'
    Get-Unbake $download
    Install-Unbake $download
    Add-ToPath
    Write-Host "unbake is in $folder"
    Write-Host ''
    & $exe setup
}
