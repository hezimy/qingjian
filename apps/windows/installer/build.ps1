<#
.SYNOPSIS
    在 Windows 上打青简安装包：release 构建产物 + 用 Inno Setup 编 qingjian.iss。
.DESCRIPTION
    在编译机（MSVC 工具链 + Inno Setup）上跑。步骤：
      1) cargo build --release 出 DLL / Server / 设置程序，再单独编一份 32 位 DLL；
      2) 从 apps\windows\server\Cargo.toml 读版本号（-dev 版接 git 短哈希）；
      3) 找 ISCC.exe（PATH 或常见安装位置）；
      4) iscc /DAppVersion=<版本> 编脚本，成品在 target\installer\qingjian-<版本>-windows-x86_64-setup.exe。
    随包数据（.qj / .tsv）直接由 .iss 从仓库 data\generated 与 assets 里取，不另建暂存目录；
    确保打包前 data\generated 里的 .qj 是最新的（bundle 流程见仓库 CLAUDE.md）。
    uiAccess 跟着 -Sign 走，不用手设 QINGJIAN_UIACCESS（见 -Sign）。
.PARAMETER SkipBuild
    跳过 cargo build（数据或 .iss 改了、二进制没变时重编安装包用）。
.PARAMETER Sign
    自签产物（sign-local.ps1）并开 uiAccess（候选窗才能盖过商店 / 任务栏搜索）。uiAccess=true 的 exe 要签名证书
    受信任才准启动，所以自签包会把证书一起打进安装包、安装时导入目标机的受信任根（卸载时撤掉）。成品名带
    -selfsigned 后缀。仍然只适合自用：对外分发的包不加此开关——不签名、关 uiAccess，候选窗在那几个系统界面里
    会被盖住，但任何机器都能起、也不用让用户信任一张自签根证书。
#>
[CmdletBinding()]
param([switch]$SkipBuild, [switch]$Sign)

$ErrorActionPreference = 'Stop'

# 仓库根：本脚本在 apps\windows\installer 下，往上三层是 ime\。
$Repo = (Resolve-Path (Join-Path $PSScriptRoot '..\..\..')).Path
$Iss  = Join-Path $PSScriptRoot 'qingjian.iss'

# uiAccess 跟着 -Sign 走（server\build.rs 读这个变量，改了会自动重编 Server）；理由见 -Sign 的说明。
$env:QINGJIAN_UIACCESS = if ($Sign) { '1' } else { '0' }
if ($Sign) {
    Write-Host 'uiAccess=1（-Sign：仅本机真机测，别用于对外分发）' -ForegroundColor Yellow
} else {
    Write-Host 'uiAccess=0（对外分发：Server 任何机器都能起；候选窗在商店 / 任务栏搜索里可能被盖）' -ForegroundColor Cyan
}

# 图标靠 rc.exe 编（apps\windows\*\build.rs 的 embed_icon）：winresource 找不到它时只警告、图标就没嵌进去，
# 在资源管理器 / 任务栏里显示白框。本机 SDK 装在 F 盘（注册表 KitsRoot10 指向 F:\Windows Kits\10\），
# 照它把 x64 目录补进 PATH；vcvars 那套在这里用不了（cmd /c 被禁）。
$kitsRoot = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots' -ErrorAction SilentlyContinue).KitsRoot10
$rc = if ($kitsRoot) {
    Get-ChildItem (Join-Path $kitsRoot 'bin') -Directory -ErrorAction SilentlyContinue |
        Sort-Object Name -Descending |
        ForEach-Object { Join-Path $_.FullName 'x64\rc.exe' } |
        Where-Object { Test-Path $_ } |
        Select-Object -First 1
}
if ($rc) { $env:PATH = "$(Split-Path $rc);$env:PATH" } else { Write-Host '警告：没找到 Windows SDK 的 rc.exe，exe 图标会缺' -ForegroundColor Yellow }

# 1) 构建产物：Server、DLL（64 / 32 位）、设置程序。
if (-not $SkipBuild) {
    Write-Host '构建 release 产物…' -ForegroundColor Cyan
    Push-Location $Repo
    try {
        cargo build --release --locked -p qingjian-windows-server -p qingjian-windows-tsf -p qingjian-settings
        if ($LASTEXITCODE -ne 0) { throw "cargo build 失败（退出码 $LASTEXITCODE）" }
        cargo build --release --locked -p qingjian-windows-tsf --target i686-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) { throw "32 位 DLL cargo build 失败（退出码 $LASTEXITCODE）" }
    } finally { Pop-Location }
}

# 缺一个产物就早报错。
$targets = @(
    'release\qingjian_tsf.dll',
    'i686-pc-windows-msvc\release\qingjian_tsf.dll',
    'release\qingjian-server.exe',
    'release\qingjian-settings.exe'
)
foreach ($t in $targets) {
    $p = Join-Path $Repo "target\$t"
    if (-not (Test-Path $p)) { throw "缺产物 $p，先跑一次不带 -SkipBuild 的构建" }
}

# 图标没嵌进去在资源管理器 / 任务栏里就是白框：装包前逐个查，别静默出一个白框包。
Add-Type -Namespace Qingjian -Name IconApi -MemberDefinition '[DllImport("shell32.dll", CharSet = CharSet.Unicode)] public static extern int ExtractIconEx(string file, int index, System.IntPtr[] large, System.IntPtr[] small, int count);'
foreach ($t in 'release\qingjian-server.exe', 'release\qingjian-settings.exe') {
    $p = Join-Path $Repo "target\$t"
    if ([Qingjian.IconApi]::ExtractIconEx($p, -1, $null, $null, 0) -eq 0) {
        throw "$t 里没有图标（rc.exe 缺失一类的嵌图标失败），先修好再打包"
    }
}

# 1.5) 签名（必须在 iscc 打包前：Inno 把已签的文件原样拷进安装包）。
if ($Sign) {
    Write-Host '自签产物（uiAccess 要求 Server 代码签名）…' -ForegroundColor Cyan
    $binaries = $targets | ForEach-Object { Join-Path $Repo "target\$_" }
    & (Join-Path $PSScriptRoot 'sign-local.ps1') -Path $binaries
}

# 2) 从 server 的 Cargo.toml 读版本（apps\* 各自写死版本，不跟 workspace）。
$cargoToml = Get-Content (Join-Path $Repo 'apps\windows\server\Cargo.toml')
$verLine = $cargoToml | Where-Object { $_ -match '^\s*version\s*=\s*"(.+)"' } | Select-Object -First 1
if (-not ($verLine -match '"(.+)"')) { throw '在 server\Cargo.toml 里没找到 version' }
$Version = $Matches[1]
# 开发版接 git 短哈希（0.1.3-dev-1a2b3c4，脏加 +），有 bug 能定位到哪次改动；发版提交去掉 -dev 就不接。
if ($Version.EndsWith('-dev')) {
    Push-Location $Repo
    try {
        $rev = (git rev-parse --short HEAD 2>$null)
        if ($LASTEXITCODE -eq 0 -and $rev) {
            if (git status --porcelain 2>$null) { $rev = "$rev+" }
            $Version = "$Version-$rev"
        }
    } finally { Pop-Location }
}
# Inno 的 VersionInfoVersion 只认数字：去掉 -alpha.1 这类预发布后缀。
$VersionNumeric = $Version -replace '-.*$', ''
Write-Host "版本 $Version" -ForegroundColor Cyan

# 3) 找 ISCC.exe：先 7（与开发机 / CI 同版本，见 docs\notes\release.md；CI 镜像 PATH 上自带 Chocolatey 的 6，
#    不带简中翻译，不能让它抢先），再 PATH，最后 6。QINGJIAN_ISCC 环境变量可直接指定。
#    7 有三个常见落点：管理员装进 Program Files（含 x86 那份）、**按当前用户装进 %LOCALAPPDATA%\Programs**
#    （winget 与安装器的用户级默认）。漏掉最后一个会让"明明装了 7"的机器退到 6——而 6.2 认不出
#    `ArchitecturesAllowed=x64compatible`（6.3 才有）会直接编译失败。
$iscc = $env:QINGJIAN_ISCC
if (-not $iscc) {
    $candidates = @(
        "${env:ProgramFiles}\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 7\ISCC.exe",
        "${env:LOCALAPPDATA}\Programs\Inno Setup 7\ISCC.exe"
    )
    $iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { $iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source }
if (-not $iscc) {
    $candidates = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles}\Inno Setup 6\ISCC.exe"
    )
    $iscc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { throw '找不到 ISCC.exe：装 Inno Setup 7 或用 QINGJIAN_ISCC 指定' }
Write-Host "用 $iscc" -ForegroundColor Cyan

# 4) 编安装包。自签包把证书一起打进去（安装时导入目标机的受信任根），成品名带 -selfsigned 以示区别。
$defines = @("/DAppVersion=$Version", "/DAppVersionNumeric=$VersionNumeric")
$suffix = ''
if ($Sign) {
    if (-not (Test-Path (Join-Path $Repo 'target\installer\qingjian-codesign.cer'))) {
        throw '缺 target\installer\qingjian-codesign.cer：sign-local.ps1 没导出证书'
    }
    $defines += '/DTrustDevCert=1'
    $suffix = '-selfsigned'
}
& $iscc @defines $Iss
if ($LASTEXITCODE -ne 0) { throw "iscc 失败（退出码 $LASTEXITCODE）" }

$out = Join-Path $Repo "target\installer\qingjian-$Version-windows-x86_64-setup$suffix.exe"
Write-Host "完成：$out" -ForegroundColor Green
