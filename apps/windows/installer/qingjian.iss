; 青简 Windows 输入法安装脚本（Inno Setup）。
;
; 装到 Program Files\Qingjian（64 位），把 TSF DLL（64 位与 32 位各一份，见 README「安装布局」）、Server、设置程序与随包数据装在一起，
; 然后：① 给安装目录加 ALL APPLICATION PACKAGES 读+执行权限（UWP/AppContainer 应用——任务栏搜索、
; 设置——才能加载 DLL）；② regsvr32 注册文本服务，64 位与 32 位各注册一次（图标落到 %ProgramData%\Qingjian）；
; ③ 在「启动」文件夹放 Server 快捷方式（登录时由 Explorer 走 ShellExecute 拉起，uiAccess 才生效——
;    计划任务直接拉起拿不到 uiAccess）；④ 装完点 Finish 立即以原用户 ShellExecute 起一次 Server，免得先注销。
; 卸载反向：删旧任务（若有）、杀 Server、反注册 DLL，再删文件（用户数据 %APPDATA%\Qingjian 保留；启动快捷方式 Inno 自动删）。
;
; 升级：DLL 被加载进每个应用进程，文件锁着覆盖不了，所以 DLL 按版本起名（qingjian_tsf-<版本>.dll）并排装，
; 注册新的，旧的装完后删（删不掉的登记成重启后删）；已开着的应用继续用旧 DLL 直到重启，Server 两个版本都服务。
; Inno 的 CloseApplications 会用 Restart Manager 找出所有加载了 *.dll 的进程要求关闭——对输入法 DLL 就是关一切，故关掉；
; 只有 Server 与设置程序这两个 exe 要覆盖，安装前自己 taskkill。
; Server 杀掉后，正在打字的应用里 DLL 连不上会自己把它拉起来（见 tsf 的 launch.rs），又占住 exe 和 mmap 着的数据：
; 安装 / 卸载期间持有互斥体 Global\QingjianInstaller，新 DLL 看到它就不拉；旧版 DLL 不认得它，
; 所以 Server exe 改名腾位、[Files] 里排最后装，新 exe 落地前 DLL 拉不起任何 Server。
; 顺序是「先按原名杀 → 再改名腾位」，不能反：改名之后进程报告的映像名仍是 qingjian-server.exe，
; 拿新文件名字去 taskkill 匹配不到，Server 活着 → 它 mmap 着的 dict.qj 删不掉 → 安装中途报错误 5。
;
; 杀要杀全：按原名杀之外，安装目录里其他 `qingjian-server*.exe` 也照样各发一遍 taskkill——
; 万一有人直接双击过 `qingjian-server.old-*.exe`，那个进程的映像名才是 .old- 名，按原名杀不到它，
; 它就是下一个「占着管道与数据文件、装完也不生效」的漏网之鱼。
; 杀完还要确认映像真的释放（删得掉那个改名后的 exe 才算），等不到就重试，最后记一行日志：那是要重启才换得掉。
;
; 版本号由打包脚本用 /DAppVersion=... 传入，缺省 0.1.0。用法见本目录 README.md。

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
; VersionInfoVersion 只认 a.b.c.d 数字；版本带预发布后缀（0.1.0-alpha.1）时由打包脚本传去掉后缀的数字版本。
#ifndef AppVersionNumeric
  #define AppVersionNumeric AppVersion
#endif
#define AppName "青简"
#define Publisher "青简"
#define WebsiteUrl "https://github.com/hezimy/qingjian"
; 脚本相对仓库根（ime/）：installer → windows → apps → ime
#define Repo "..\..\.."
; 按版本起名的 TSF DLL（见文件头「升级」）。
#define TsfDll "qingjian_tsf-" + AppVersion + ".dll"
#define TsfDll32 "qingjian_tsf-" + AppVersion + "-x86.dll"
; 自签包（build.ps1 -Sign）：Server 带 uiAccess manifest，只有证书受信任 + 装在 Program Files 才起得来，
; 所以证书必须随包安装。成品文件名带 -selfsigned，免得和对外发布的包混在一起。
#ifdef TrustDevCert
  #define OutputBaseFilename "qingjian-" + AppVersion + "-windows-x86_64-setup-selfsigned"
  #define CertSubject "CN=Qingjian Dev CodeSign"
#else
  #define OutputBaseFilename "qingjian-" + AppVersion + "-windows-x86_64-setup"
#endif

[Setup]
AppId={{A7E3C1F2-5B94-4D6A-9C0E-2F8B1D3A6E70}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#Publisher}
AppSupportURL={#WebsiteUrl}
VersionInfoVersion={#AppVersionNumeric}
DefaultDirName={autopf}\Qingjian
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; 系统要求 Windows 10 1809 / Windows 11（见 docs\user\getting-started\install.md）。
MinVersion=10.0.17763
PrivilegesRequired=admin
; 别让 Restart Manager 去关所有加载了 DLL 的应用（那是每一个有文本框的应用）。
CloseApplications=no
OutputDir={#Repo}\target\installer
OutputBaseFilename={#OutputBaseFilename}
SetupIconFile={#Repo}\apps\windows\tsf\resources\qingjian.ico
UninstallDisplayIcon={app}\qingjian.ico
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "chs"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

[Messages]
; 完成页：DLL 会装进每个应用进程，装之前就开着的应用要用新版必须重启（见文件头「升级」）。
; 这是最容易被误解成「设置/新版没生效」的一点，所以放在完成页明说。
FinishedLabel=安装完成，后台服务已按新版本重新启动。%n%n已经开着没关的应用（浏览器、WPS、编辑器等）里加载的还是旧版 DLL，要关掉再重新打开才会用上新版；嫌麻烦的话，注销后重新登录（或重启电脑）能一次把所有应用都换上。

[Files]
; —— 二进制 ——
; DLL 按版本起名并排装；卸载时若仍被占用，登记成重启后删。
Source: "{#Repo}\target\release\qingjian_tsf.dll";      DestDir: "{app}"; DestName: "{#TsfDll}"; Flags: ignoreversion uninsrestartdelete
Source: "{#Repo}\target\i686-pc-windows-msvc\release\qingjian_tsf.dll"; DestDir: "{app}"; DestName: "{#TsfDll32}"; Flags: ignoreversion uninsrestartdelete
Source: "{#Repo}\target\release\qingjian-settings.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Repo}\apps\windows\tsf\resources\qingjian.ico"; DestDir: "{app}"; Flags: ignoreversion
; —— 随包生成数据（只装运行时要的 .qj / .tsv，不装 dev 中间产物）——
Source: "{#Repo}\data\generated\dict.qj";        DestDir: "{app}\data\generated";       Flags: ignoreversion
Source: "{#Repo}\data\generated\lm.qj";          DestDir: "{app}\data\generated";       Flags: ignoreversion
Source: "{#Repo}\data\generated\glossary-en.qj"; DestDir: "{app}\data\generated";       Flags: ignoreversion
Source: "{#Repo}\data\generated\glossary-ja.qj"; DestDir: "{app}\data\generated";       Flags: ignoreversion
Source: "{#Repo}\data\generated\glossary-zh.qj"; DestDir: "{app}\data\generated";       Flags: ignoreversion
Source: "{#Repo}\data\generated\glossary-es.qj"; DestDir: "{app}\data\generated";       Flags: ignoreversion
Source: "{#Repo}\data\generated\glossary-vi.qj"; DestDir: "{app}\data\generated";       Flags: ignoreversion
Source: "{#Repo}\data\generated\glossary-de.qj"; DestDir: "{app}\data\generated";       Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Repo}\data\generated\english.tsv";    DestDir: "{app}\data\generated";       Flags: ignoreversion
Source: "{#Repo}\data\generated\dicts\*.qj";     DestDir: "{app}\data\generated\dicts";  Flags: ignoreversion
; —— 随包辅码码表（笔画，开箱可用）：Server 扫 data\generated\codes\；CNS11643 筆順資料派生，署名见「关于」页 ——
;    缺表时不阻塞打包（skipifsourcedoesntexist），但发布前应先跑 dict-convert pack codes 生成它
Source: "{#Repo}\data\generated\codes\*.qj";   DestDir: "{app}\data\generated\codes"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Repo}\assets\stroke\LICENSE-CNS11643.txt"; DestDir: "{app}\data\generated\codes"; Flags: ignoreversion
; —— 含章·知微与含章·通变模型 ——
Source: "{#Repo}\data\models\hanzhang-zhiwei\hanzhang-zhiwei-small.qjm"; DestDir: "{app}\data\models\hanzhang-zhiwei"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#Repo}\data\models\hanzhang-tongbian\hanzhang-tongbian-small.qjm"; DestDir: "{app}\data\models\hanzhang-tongbian"; Flags: ignoreversion skipifsourcedoesntexist
; —— 随 git 的资源 ——
Source: "{#Repo}\assets\emoji\emoji-zh.tsv";     DestDir: "{app}\assets\emoji";  Flags: ignoreversion
Source: "{#Repo}\assets\emoji\emoji-en.tsv";     DestDir: "{app}\assets\emoji";  Flags: ignoreversion
Source: "{#Repo}\assets\levels\levels-en.tsv";   DestDir: "{app}\assets\levels"; Flags: ignoreversion
Source: "{#Repo}\assets\levels\levels-ja.tsv";   DestDir: "{app}\assets\levels"; Flags: ignoreversion
; 五笔码表（输入方案选五笔时用，见 assets/wubi/README.md；极点 86 码表，Apache-2.0）。
; 走 assets\ 与 emoji / levels 一致，Server 的 `dispatch::code::find_code_table` 照同一个相对路径找，
; 开发布局（cargo run）也对得上
Source: "{#Repo}\assets\wubi\wubi86.tsv";        DestDir: "{app}\assets\wubi";   Flags: ignoreversion
Source: "{#Repo}\assets\sample\dict.tsv";        DestDir: "{app}\assets\sample"; Flags: ignoreversion
; —— Server 放最后：它一落地，旧版 DLL 就能把它拉起来并占住数据文件（见文件头）——
Source: "{#Repo}\target\release\qingjian-server.exe";   DestDir: "{app}"; Flags: ignoreversion
; 自签代码签名证书（只含公钥；build.ps1 -Sign 让 sign-local.ps1 导出到这里）。
#ifdef TrustDevCert
Source: "{#Repo}\target\installer\qingjian-codesign.cer"; DestDir: "{app}"; Flags: ignoreversion
#endif

[Icons]
Name: "{group}\青简设置"; Filename: "{app}\qingjian-settings.exe"; IconFilename: "{app}\qingjian.ico"
Name: "{group}\卸载青简"; Filename: "{uninstallexe}"
; 登录自启：登录时 Explorer 走 ShellExecute 拉起本快捷方式 → AppInfo 授予 uiAccess，候选窗才能盖过商店 / 任务栏搜索。
; 用 {commonstartup}（所有用户「启动」文件夹）而非 {userstartup}：本安装器是 admin 机器级安装，
; admin 模式下写每用户区会落到「谁提权就写谁」的 profile（Inno 会告警且可能不是目标用户）；
; 机器级「启动」项对每个登录用户都在其会话里由该用户的 Explorer 拉起，仍是 per-user 运行、仍授予 uiAccess。
; （计划任务直接拉起拿不到 uiAccess，故不用 schtasks。）
Name: "{commonstartup}\青简 Server"; Filename: "{app}\qingjian-server.exe"; WorkingDir: "{app}"; IconFilename: "{app}\qingjian.ico"

[Run]
; ① UWP/AppContainer 应用要能读安装目录才能加载 DLL（*S-1-15-2-1 = ALL APPLICATION PACKAGES，按 SID 与语言无关）。
Filename: "{sys}\icacls.exe"; Parameters: """{app}"" /grant *S-1-15-2-1:(OI)(CI)RX /T /C /Q"; \
  Flags: runhidden waituntilterminated; StatusMsg: "配置应用容器权限…"
#ifdef TrustDevCert
; ①.5 信任自签证书：uiAccess 的 exe 只有签名证书受信任才起得来（否则报「从服务器返回了一个参照」，Server 直接起不来）。
;     只对 -Sign 打的自签包生效；对外发布的包里没有这条，也没有证书文件。
Filename: "{sys}\certutil.exe"; Parameters: "-addstore -f Root ""{app}\qingjian-codesign.cer"""; \
  Flags: runhidden waituntilterminated; StatusMsg: "信任自签证书…"
Filename: "{sys}\certutil.exe"; Parameters: "-addstore -f TrustedPublisher ""{app}\qingjian-codesign.cer"""; \
  Flags: runhidden waituntilterminated
#endif
; ② 注册文本服务（写 HKCR + 图标到 %ProgramData%\Qingjian\qingjian.ico）。注册的是本版本的 DLL，
;    InprocServer32 指向新文件；旧版本的 DLL **不能** regsvr32 /u（那会把整个 CLSID 注销掉）。
Filename: "{sys}\regsvr32.exe"; Parameters: "/s ""{app}\{#TsfDll}"""; \
  Flags: runhidden waituntilterminated; StatusMsg: "注册输入法…"
; 32 位那份用 SysWOW64 里的 32 位 regsvr32 注册，InprocServer32 才落到 WOW6432Node 下给 32 位进程用。
Filename: "{syswow64}\regsvr32.exe"; Parameters: "/s ""{app}\{#TsfDll32}"""; \
  Flags: runhidden waituntilterminated; StatusMsg: "注册输入法（32 位）…"
; ④ 装完立即起一次 Server 见 [Code] 的 NextButtonClick：uiAccess=true 的 exe 不能用
;    CreateProcess / runasoriginaluser 拉起（报 740），必须走 ShellExecute（等同双击）。

[UninstallRun]
; 反向：先删登录任务、杀 Server / 设置程序、反注册 DLL，Inno 再删文件（DLL 若仍被占用，重启后删）。
Filename: "{sys}\schtasks.exe"; Parameters: "/delete /tn ""Qingjian Server"" /f"; \
  Flags: runhidden; RunOnceId: "DelLogonTask"
Filename: "{sys}\taskkill.exe"; Parameters: "/im qingjian-server.exe /f"; \
  Flags: runhidden; RunOnceId: "KillServer"
Filename: "{sys}\taskkill.exe"; Parameters: "/im qingjian-settings.exe /f"; \
  Flags: runhidden; RunOnceId: "KillSettings"
Filename: "{sys}\regsvr32.exe"; Parameters: "/u /s ""{app}\{#TsfDll}"""; \
  Flags: runhidden; RunOnceId: "UnregDll"
Filename: "{syswow64}\regsvr32.exe"; Parameters: "/u /s ""{app}\{#TsfDll32}"""; \
  Flags: runhidden; RunOnceId: "UnregDll32"
#ifdef TrustDevCert
; 撤掉自签包导入的根证书（按主题删，只删这张自签证书；装别的版本没人再签同一主题）。
Filename: "{sys}\certutil.exe"; Parameters: "-delstore Root ""{#CertSubject}"""; \
  Flags: runhidden; RunOnceId: "DelDevCertRoot"
Filename: "{sys}\certutil.exe"; Parameters: "-delstore TrustedPublisher ""{#CertSubject}"""; \
  Flags: runhidden; RunOnceId: "DelDevCertPublisher"
#endif

[InstallDelete]
; 更早版本装的旧版设置程序（新包已不带它），升级时顺手清掉
Type: files; Name: "{app}\qingjian-settings-legacy.exe"
; 更早版本装在当前用户「启动」文件夹里的自启快捷方式：与机器级那份并存会起两个 Server（两条状态条）。
Type: files; Name: "{userstartup}\Qingjian Server.lnk"
; 更早版本装的模型三件套（现在只带正式命名的 .qjm）：留着白占 56 MB。
Type: files; Name: "{app}\data\model\model.safetensors"
Type: files; Name: "{app}\data\model\config.json"
Type: files; Name: "{app}\data\model\vocab.json"
; 0.1.4 开发版的随包码表旧位置
Type: filesandordirs; Name: "{app}\codes"

[UninstallDelete]
; 历次升级留下的旧版本 DLL（正常在升级时就删了；仍被占用的会留到这里）。
Type: files; Name: "{app}\qingjian_tsf-*.dll"
#ifdef TrustDevCert
Type: files; Name: "{app}\qingjian-codesign.cer"
#endif
Type: files; Name: "{app}\qingjian-server.old-*.exe"

[Code]
function CreateMutex(Attributes: Longint; InitialOwner: BOOL; Name: String): THandle;
  external 'CreateMutexW@kernel32.dll stdcall';

{ 安装 / 卸载期间持有的互斥体，名字与 tsf 的 launch.rs 一致；句柄不关，进程退出时系统收回。 }
procedure HoldInstallerMutex;
begin
  if CreateMutex(0, False, 'Global\QingjianInstaller') = 0 then
    Log('建安装互斥体失败');
end;

function InitializeSetup: Boolean;
begin
  HoldInstallerMutex;
  Result := True;
end;

function InitializeUninstall: Boolean;
begin
  HoldInstallerMutex;
  Result := True;
end;

{ 运行中的 exe 不能覆盖但能改名：改成 qingjian-server.old-<随机>.exe 腾出名字，旧版 DLL 就拉不起它（见文件头）。
  装完由 DeleteStaleFiles 删掉；返回改名后的路径（没得改 / 改名失败返回空串）。 }
function RetireServerExe: String;
var
  Path, Retired: String;
begin
  Result := '';
  Path := ExpandConstant('{app}\qingjian-server.exe');
  if FileExists(Path) then
  begin
    Retired := ExpandConstant('{app}\qingjian-server.old-') + IntToStr(Random(1000000)) + '.exe';
    if RenameFile(Path, Retired) then
      Result := Retired
    else
      Log('改名旧 Server 失败: ' + Path);
  end;
end;

procedure KillProcess(const Image: String);
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/im ' + Image + ' /f', '',
    SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

{ 补充手段：按安装目录里现存的文件名逐个 taskkill，覆盖「有人直接双击了
  qingjian-server.old-*.exe」这种映像名真就是 .old- 名的情形。**主力是按原名杀**（见 StopServers）：
  进程报告的映像名是装载那一刻的文件名，改成 .old-*.exe 之后拿新文件名字去 taskkill 一个也匹配不到。 }
procedure KillImagesInApp(const Prefix: String);
var
  Dir: String;
  Found: TFindRec;
begin
  Dir := ExpandConstant('{app}');
  if FindFirst(Dir + '\' + Prefix + '*.exe', Found) then
  begin
    try
      repeat
        KillProcess(Found.Name);
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
end;

{ 等映像文件真的释放：进程还转着时它的映像段把文件锁着，删不掉。删掉了（或本来就没有）就是释放了。 }
function WaitForImageRelease(const Path: String; Tries: Integer): Boolean;
begin
  Result := True;
  while Tries > 0 do
  begin
    if not FileExists(Path) then
      Exit;
    if DeleteFile(Path) then
      Exit;
    Sleep(100);
    Tries := Tries - 1;
  end;
  Result := False;
end;

{ 升级前把服务停干净：先按原名杀 → 再把 Server exe 改名腾位（防旧 DLL 拉起来）→ 等映像真释放，等不到重来一轮。
  返回是否真的停干净；false 表示这次要重启才换得掉（已记一行日志，见文件头）。

  **务必按原名杀**：磁盘上把 qingjian-server.exe 改名成 .old-*.exe 之后，进程报告的映像名
  仍然是 qingjian-server.exe（Windows 记的是装载那一刻的文件名，实测改名后用新名字 taskkill 匹配不到、
  用原名一击即中）。按磁盘文件名去杀，Server 就会活下来——它 mmap 着的 dict.qj 删不掉，
  安装中途弹出「尝试替换已存在的文件时出错: DeleteFile 失败; 错误代码 5. 拒绝访问」。 }
function StopServers: Boolean;
var
  Retired: String;
  Round: Integer;
begin
  Result := False;
  Retired := '';
  for Round := 1 to 3 do
  begin
    KillProcess('qingjian-server.exe');
    KillProcess('qingjian-settings.exe');
    KillProcess('qingjian-settings-legacy.exe');
    { 第一轮改名腾位；之后若 Server 又被拉起来（新文件已落地才有可能），再改一次 }
    if (Retired = '') and FileExists(ExpandConstant('{app}\qingjian-server.exe')) then
      Retired := RetireServerExe;
    KillImagesInApp('qingjian-server');
    KillImagesInApp('qingjian-settings');
    if (Retired = '') or WaitForImageRelease(Retired, 10) then
    begin
      Result := True;
      Exit;
    end;
    Log('第 ' + IntToStr(Round) + ' 轮杀完 Server 后映像仍未释放，重试');
  end;
  Log('旧 Server 的映像一直没释放（提权 / 别的用户起的？），要重启才换得掉: ' + Retired);
end;

{ 同版本重装（开发期反复装）：目标文件名与已加载的 DLL 撞名，覆盖不了但 Windows 允许改名，
  先把它改成 qingjian_tsf-<版本>.old-<随机>.dll 腾出名字，装完由 DeleteStaleDlls 删掉 / 登记重启后删。 }
procedure RetireLoadedDll(const Name: String);
var
  Path, Retired: String;
begin
  Path := ExpandConstant('{app}\') + Name;
  if FileExists(Path) then
  begin
    Retired := ExpandConstant('{app}\qingjian_tsf-{#AppVersion}.old-') + IntToStr(Random(1000000)) + '.dll';
    if not RenameFile(Path, Retired) then
      Log('改名旧 DLL 失败: ' + Path);
  end;
end;

{ 覆盖前先结束 Server 与设置程序（只有这两个 exe 要覆盖；DLL 按版本并排装，不用关应用）。
  没在跑时 taskkill 返回非 0，忽略；停不干净会在安装日志里留一行（见 StopServers）。 }
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  if not StopServers then
    Log('本次升级：旧 Server 没停干净，装完要重启才换得掉');
  RetireLoadedDll('{#TsfDll}');
  RetireLoadedDll('{#TsfDll32}');
  Result := '';
end;

{ 清掉旧版本建的「登录自启」计划任务（现在改用「启动」文件夹快捷方式，见 [Icons]）。
  计划任务直接拉起 Server 拿不到 uiAccess，升级安装时删掉它，免得它在登录时抢先以非 uiAccess 方式
  起 Server 并占住命名管道，让快捷方式那份起不来。没有旧任务时 schtasks 返回非 0，忽略即可。 }
procedure DeleteLegacyLogonTask;
var
  ResultCode: Integer;
begin
  Exec('schtasks.exe', '/delete /tn "Qingjian Server" /f', '',
    SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

{ 删掉改名腾位的旧 Server；删不掉的登记成重启后删。 }
procedure DeleteRetiredServers;
var
  Dir, Path: String;
  Found: TFindRec;
begin
  Dir := ExpandConstant('{app}');
  if FindFirst(Dir + '\qingjian-server.old-*.exe', Found) then
  begin
    try
      repeat
        Path := Dir + '\' + Found.Name;
        if not DeleteFile(Path) then
          RestartReplace(Path, '');
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
end;

{ 删掉旧版本的 DLL（含没带版本号的最早那份）。仍被某个应用加载着的删不掉，登记成重启后删：
  那些应用重启前继续用旧 DLL，Server 两个版本都服务。 }
procedure DeleteStaleDlls;
var
  Dir, Path: String;
  Found: TFindRec;
begin
  Dir := ExpandConstant('{app}');
  if FindFirst(Dir + '\qingjian_tsf*.dll', Found) then
  begin
    try
      repeat
        if (CompareText(Found.Name, '{#TsfDll}') <> 0) and (CompareText(Found.Name, '{#TsfDll32}') <> 0) then
        begin
          Path := Dir + '\' + Found.Name;
          if not DeleteFile(Path) then
            RestartReplace(Path, '');
        end;
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    DeleteLegacyLogonTask;
    DeleteStaleDlls;
    DeleteRetiredServers;
  end;
end;

{ 卸载同理：[UninstallRun] 杀 Server 之前先改名，旧版 DLL 拉不起它，文件才删得掉。 }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Retired: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    Retired := RetireServerExe;
    if Retired <> '' then
      Log('卸载：旧 Server 已改名腾位 ' + Retired);
    KillImagesInApp('qingjian-server');
    KillImagesInApp('qingjian-settings');
  end;
end;

{ 装完在完成页点 Finish 后立即起一次 Server。
  uiAccess=true 的 exe 不能用 CreateProcess / runasoriginaluser 拉起（报 740），
  必须以原（非提升）用户身份 ShellExecute（等同双击），AppInfo 才会授予 uiAccess 高 z-band 权限。 }
function NextButtonClick(CurPageID: Integer): Boolean;
var
  ErrorCode: Integer;
begin
  Result := True;
  if (CurPageID = wpFinished) and (not WizardSilent) then
    ShellExecAsOriginalUser(
      '', ExpandConstant('{app}\qingjian-server.exe'), '', '',
      SW_SHOWNORMAL, ewNoWait, ErrorCode);
end;
