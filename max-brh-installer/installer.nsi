; M.A.X.: Beyond Rust Horizon - portable Windows installer (NSIS 3.x).
;
; Design: distro-installers.md section D (owner requirements 2026-11-28).
; Everything is installed inside the directory the user selects; no ARP registry
; entries, no system directories. The only optional system artifact is the Start
; Menu shortcut, created exclusively when its checkbox (or /SHORTCUT) is set.
;
; Custom UI: the owner's frame art is stretched over every content page, the
; install page uses the in-progress console art with the native progress bar
; placed in its slot, and the finish page uses a banner strip. BMPs are produced
; by scripts/prepare-ui.ps1; staging by scripts/stage.ps1.

Unicode true

!define PRODUCT "M.A.X.: Beyond Rust Horizon"
!define VERSION "0.1.28a"
!define SHORTCUT_FOLDER "$SMPROGRAMS\MAX - Beyond Rust Horizon"
!define SHORTCUT_LINK "$SMPROGRAMS\MAX - Beyond Rust Horizon\MAX - Beyond Rust Horizon.lnk"

!ifndef STAGE
  !define STAGE "stage\game"
!endif
!ifndef UIDIR
  !define UIDIR "ui"
!endif
!ifndef OUTFILE
  !define OUTFILE "dist\MAX-BRH-0.1.28a-setup.exe"
!endif

; Geometry of the install-page console art (resources/install_in_progress_ui.png,
; 308x162) measured on the source PNG. The decorative horizontal groove is a
; recessed slot in the middle of the panel:
;   * raised rim:     top bevel y=79..85, front lip y=106..112
;   * slot opening:   x=44..267, y=86..105  (224x20)
;   * lit inner floor x=50..266, y=89..105
; The progress fill is inset one pixel inside the opening (x=47..266, y=87..104)
; so the top bevel and the front lip stay visible on every side, and the fill
; never spills over the rim or beyond the panel.
!define PANEL_W 308
!define PANEL_H 162
!define SLOT_LEFT 47
!define SLOT_TOP 87
!define SLOT_W 220
!define SLOT_H 18

!define MUI_ICON "${UIDIR}\game-icon.ico"
!define MUI_UNICON "${UIDIR}\game-icon.ico"
!define MUI_ABORTWARNING
!define MUI_BGCOLOR "1E1912"
!define MUI_TEXTCOLOR "E8DCC3"
!define MUI_CUSTOMFUNCTION_GUIINIT BrhGuiInit
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.BrhUnGuiInit

!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "Sections.nsh"
!include "WinMessages.nsh"

Name "${PRODUCT} ${VERSION}"
Caption "${PRODUCT} ${VERSION}"
UninstallCaption "Uninstall ${PRODUCT} ${VERSION}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\MAX-BRH"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 64
SetDatablockOptimize on
CRCCheck on
XPStyle on
BrandingText " "

VIProductVersion "0.1.28.0"
VIAddVersionKey "ProductName" "${PRODUCT}"
VIAddVersionKey "FileDescription" "${PRODUCT} setup"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "Free non-commercial fan remake; game assets (c) Interplay 1996"

; --- Pages: language dialog -> disclaimer -> install folder -> components -> install -> finish

Page custom BrhDisclaimerCreate
Page custom BrhDirectoryCreate BrhDirectoryLeave
Page custom BrhComponentsCreate BrhComponentsLeave
!define MUI_PAGE_CUSTOMFUNCTION_SHOW BrhInstfilesShow
!insertmacro MUI_PAGE_INSTFILES
Page custom BrhFinishCreate BrhFinishLeave

UninstPage custom un.BrhWelcomeCreate
UninstPage custom un.BrhOptionsCreate un.BrhOptionsLeave
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.BrhUninstfilesShow
!insertmacro MUI_UNPAGE_INSTFILES
UninstPage custom un.BrhFinishCreate

!insertmacro MUI_LANGUAGE "Russian"
!insertmacro MUI_LANGUAGE "English"

; --- Language strings

LangString BrhDisclaimerTitle 1049 "ОТКАЗ ОТ ОТВЕТСТВЕННОСТИ"
LangString BrhDisclaimerTitle 1033 "DISCLAIMER"
LangString BrhDisclaimerText 1049 "Это бесплатный некоммерческий фан-ремейк игры M.A.X.: Mechanized Assault & Exploration.$\r$\n$\r$\nИгровые ассеты © Interplay 1996. Проект не связан с правообладателем и не одобрен им.$\r$\n$\r$\nИгра распространяется бесплатно, без коммерческой выгоды."
LangString BrhDisclaimerText 1033 "This is a free, non-commercial fan remake of M.A.X.: Mechanized Assault & Exploration.$\r$\n$\r$\nThe game assets are (c) Interplay 1996. This project is not affiliated with or endorsed by the rights holder.$\r$\n$\r$\nThe game is distributed free of charge and without commercial gain."

LangString BrhDirTitle 1049 "КАТАЛОГ УСТАНОВКИ"
LangString BrhDirTitle 1033 "INSTALL FOLDER"
LangString BrhDirLabel 1049 "Всё будет установлено в выбранный каталог:"
LangString BrhDirLabel 1033 "Everything will be installed into the selected folder:"
LangString BrhDirNote 1049 "Портативная установка: игра, данные и remaster остаются в этом каталоге; сейвы и настройки — здесь же.$\r$\n$\r$\nПри обновлении поверх существующей версии сейвы и настройки сохраняются."
LangString BrhDirNote 1033 "Portable install: the game, data and remaster stay in this folder; saves and settings live here too.$\r$\n$\r$\nWhen upgrading an existing installation, saves and settings are preserved."
LangString BrhDirUpgrade 1049 "Найдена существующая установка: файлы игры будут обновлены, сейвы и настройки сохранены."
LangString BrhDirUpgrade 1033 "Existing installation found: game files will be updated, saves and settings are kept."
LangString BrhBrowse 1049 "Обзор..."
LangString BrhBrowse 1033 "Browse..."
LangString BrhSelectFolder 1049 "Выберите каталог установки"
LangString BrhSelectFolder 1033 "Select the install folder"
LangString BrhDirEmpty 1049 "Укажите каталог установки."
LangString BrhDirEmpty 1033 "Please specify an install folder."

LangString BrhCompTitle 1049 "КОМПОНЕНТЫ"
LangString BrhCompTitle 1033 "COMPONENTS"
LangString BrhGameComp 1049 "Игра (обязательно)"
LangString BrhGameComp 1033 "Game (required)"
LangString BrhGameCompNote 1049 "Данные игры, обязательный HD-remaster, музыка и видео — около 430 МБ."
LangString BrhGameCompNote 1033 "Game data, the mandatory HD remaster, music and videos - about 430 MB."
LangString BrhShortcut 1049 "Создать ярлык в меню «Пуск»"
LangString BrhShortcut 1033 "Create a Start Menu shortcut"
LangString BrhShortcutNote 1049 "Ярлык создаётся только при установленном флажке; других записей в системе нет."
LangString BrhShortcutNote 1033 "The shortcut is created only when this box is checked; no other system entries are made."

LangString BrhFinishTitle 1049 "УСТАНОВКА ЗАВЕРШЕНА"
LangString BrhFinishTitle 1033 "INSTALLATION COMPLETE"
LangString BrhFinishText 1049 "M.A.X.: Beyond Rust Horizon ${VERSION} установлена.$\r$\n$\r$\nФайлы игры: $INSTDIR$\r$\nСейвы и настройки будут храниться в этом же каталоге."
LangString BrhFinishText 1033 "M.A.X.: Beyond Rust Horizon ${VERSION} has been installed.$\r$\n$\r$\nGame files: $INSTDIR$\r$\nSaves and settings will be stored in this folder."
LangString BrhRunGame 1049 "Запустить игру"
LangString BrhRunGame 1033 "Launch the game"
LangString BrhInstalling 1049 "Установка игры и обязательного HD-remaster..."
LangString BrhInstalling 1033 "Installing the game and the mandatory HD remaster..."

LangString BrhUninstTitle 1049 "УДАЛЕНИЕ M.A.X.: BEYOND RUST HORIZON"
LangString BrhUninstTitle 1033 "REMOVE M.A.X.: BEYOND RUST HORIZON"
LangString BrhUninstText 1049 "Будут удалены файлы игры, данные и remaster. $\r$\n$\r$\nСейвы и сохранения кампаний по умолчанию остаются в каталоге; файлы настроек удаляются вместе с игрой."
LangString BrhUninstText 1033 "Game files, data and the remaster will be removed.$\r$\n$\r$\nSaves and campaign progress are kept in the folder by default; settings files are removed with the game."
LangString BrhDeleteSaves 1049 "Удалить также сейвы и сохранения"
LangString BrhDeleteSaves 1033 "Also delete save games and campaign progress"
LangString BrhDeleteSavesNote 1049 "По умолчанию сейвы сохраняются. Файлы настроек (settings.jsonc, preferences.ini) удаляются вместе с игрой."
LangString BrhDeleteSavesNote 1033 "Saves are kept by default. Settings files (settings.jsonc, preferences.ini) are removed with the game."
LangString BrhUninstDoneTitle 1049 "УДАЛЕНИЕ ЗАВЕРШЕНО"
LangString BrhUninstDoneTitle 1033 "UNINSTALL COMPLETE"
LangString BrhUninstDoneText 1049 "Файлы игры удалены.$\r$\n$\r$\nСейвы сохранены в $INSTDIR."
LangString BrhUninstDoneText 1033 "The game files have been removed.$\r$\n$\r$\nSaves were kept in $INSTDIR."
LangString BrhUninstDoneTextDeleted 1049 "Файлы игры и сейвы удалены."
LangString BrhUninstDoneTextDeleted 1033 "Game files and saves were removed."
LangString BrhRemoving 1049 "Удаление файлов игры..."
LangString BrhRemoving 1033 "Removing game files..."

LangString SecGame 1049 "Игра"
LangString SecGame 1033 "Game"
LangString SecShortcut 1049 "Ярлык в меню «Пуск»"
LangString SecShortcut 1033 "Start Menu shortcut"

; --- Global variables

Var hPage
Var hBg
Var hCard
Var hTitle
Var hBody
Var hNote
Var hEdit
Var hBrowse
Var hChkGame
Var hChkShortcut
Var hRunGame
Var hFontTitle
Var hFontBody
Var hBmpFrame
Var hBmpCard
Var hBmpExtra
Var hPanel
Var hPanelBmp
Var hInstStatus
Var PageW
Var PageH
Var PanelY
Var CreateShortcut
Var DeleteSaves
Var hBarFill
Var hBarFillBmp
Var BarFillX
Var BarFillY

; --- Initialization

Function BrhCreateFonts
    System::Call 'gdi32::CreateFont(i -17, i 0, i 0, i 0, i 700, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t "Segoe UI") p.r0'
    StrCpy $hFontTitle $0
    System::Call 'gdi32::CreateFont(i -13, i 0, i 0, i 0, i 400, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t "Segoe UI") p.r0'
    StrCpy $hFontBody $0
FunctionEnd

Function BrhGuiInit
    SetCtlColors $HWNDPARENT 0xFFFFFF 0x1E1912
    Call BrhCreateFonts
FunctionEnd

Function un.BrhUnGuiInit
    SetCtlColors $HWNDPARENT 0xFFFFFF 0x1E1912
    Call un.BrhCreateFonts
FunctionEnd

Function .onInit
    SetShellVarContext current

    ; Auto-advance to the finish page. AutoClose makes the wizard leave the install
    ; page by itself as soon as every section has really finished, instead of
    ; waiting for a Next click. MUI normally sets this in its own finish page, but
    ; this installer uses a custom nsDialogs finish page, so the flag is set here
    ; explicitly. It only affects the install page: the finish page stays open.
    SetAutoClose true

    ; Language selection (RU/EN) before the wizard pages. MUI's dialog keeps the
    ; choice in memory only, so the portable install still writes no registry.
    !insertmacro MUI_LANGDLL_DISPLAY

    ; /SHORTCUT: opt-in shortcut for unattended installs. The GUI checkbox sets
    ; $CreateShortcut; the default is no shortcut.
    StrCpy $CreateShortcut "0"
    StrCpy $0 $CMDLINE
    StrCpy $1 "/SHORTCUT"
    Call BrhHasSwitch
    ${If} $2 == "1"
        StrCpy $CreateShortcut "1"
    ${EndIf}
FunctionEnd

Function un.onInit
    SetShellVarContext current

    StrCpy $DeleteSaves "0"
    StrCpy $0 $CMDLINE
    StrCpy $1 "/SAVES"
    Call un.BrhHasSwitch
    ${If} $2 == "1"
        StrCpy $DeleteSaves "1"
    ${EndIf}
FunctionEnd

; Case-sensitive substring scan for a valueless command-line switch. FileFunc's
; GetOptions returns an empty result for switches without a value, so the
; portable installer parses its own /SHORTCUT and /SAVES flags.
Function BrhHasSwitch
    StrCpy $2 0
    StrCpy $3 0
    StrLen $6 $1
    ${Do}
        StrCpy $4 $0 1 $3
        ${If} $4 == ""
            ${ExitDo}
        ${EndIf}
        StrCpy $5 $0 $6 $3
        ${If} $5 == $1
            StrCpy $2 1
            ${ExitDo}
        ${EndIf}
        IntOp $3 $3 + 1
    ${Loop}
FunctionEnd

Function un.BrhHasSwitch
    StrCpy $2 0
    StrCpy $3 0
    StrLen $6 $1
    ${Do}
        StrCpy $4 $0 1 $3
        ${If} $4 == ""
            ${ExitDo}
        ${EndIf}
        StrCpy $5 $0 $6 $3
        ${If} $5 == $1
            StrCpy $2 1
            ${ExitDo}
        ${EndIf}
        IntOp $3 $3 + 1
    ${Loop}
FunctionEnd

; --- Shared custom-page helpers

Function BrhHideChrome
    ; The whole window is our skin: hide MUI header, header image, background
    ; statics and the branding text.
    StrCpy $R0 1028
    ${Do}
        GetDlgItem $R1 $HWNDPARENT $R0
        ${If} $R1 != 0
            ShowWindow $R1 0
        ${EndIf}
        IntOp $R0 $R0 + 1
    ${LoopUntil} $R0 > 1050

    GetDlgItem $R1 $HWNDPARENT 1256
    ${If} $R1 != 0
        ShowWindow $R1 0
    ${EndIf}
FunctionEnd

Function BrhFitPage
    System::Call 'user32::GetClientRect(p$HWNDPARENT, @r1)'
    System::Call '*$1(i,i,i.r2,i.r3)'
    StrCpy $PageW $2
    IntOp $PageH $3 - 42

    ; Fit exactly above the native navigation buttons, whatever the DPI.
    GetDlgItem $4 $HWNDPARENT 1
    ${If} $4 != 0
        System::Call 'user32::GetWindowRect(p$4, @r5)'
        System::Call 'user32::MapWindowPoints(p0, p$HWNDPARENT, pr5, i2)'
        System::Call '*$5(i,i.r6,i,i)'
        ${If} $6 > 80
        ${AndIf} $6 < $3
            IntOp $PageH $6 - 8
        ${EndIf}
    ${EndIf}

    ${If} $PageH < 100
        IntOp $PageH $3 - 42
    ${EndIf}

    System::Call 'user32::SetWindowPos(p$hPage, p0, i0, i0, i$PageW, i$PageH, i0x14)'
FunctionEnd

; Full-page background shared by every page (custom nsDialogs pages and the two
; instfiles pages). The frame art is scaled to the page rectangle with
; LoadAndSetImage /RESIZETOFIT, so its pixels map linearly onto 0..PageW and
; 0..PageH. The M.A.X. inscription is part of that art and is centred in the
; source image (640x480): the black bar under the tagline spans x=230..408, i.e.
; its centre is 319..320 - the image centre. Scaling therefore puts the
; inscription on PageW / 2 on every page, at any window size and DPI.
;
; A plain SS_BITMAP static (STM_SETIMAGE without /RESIZETOFIT) must not be used
; here: such a static ignores the requested width/height and resizes itself to
; the 640x480 bitmap, so the art is painted 1:1 from the page origin - cropped on
; the right when the page is narrower than the art and left-aligned (artwork off
; centre, bare strip on the right) when the page is wider, e.g. at a large DPI.
;
; WS_CLIPSIBLINGS is required as well: the background is the bottom-most child,
; and a static without it repaints over overlapping siblings even when they sit
; above it in z-order - which hid the content card, the banner and the check
; boxes on every custom page.
!macro BrhPageBg
    System::Call 'user32::CreateWindowEx(i0, t "STATIC", t "", i0x5400000E, i0, i0, i$PageW, i$PageH, p$hPage, p0, p0, p0) p.r5'
    ${NSD_SetStretchedBitmap} $5 "${UIDIR}\frame.bmp" $6
    StrCpy $hBg $5
    StrCpy $hBmpFrame $6
!macroend

Function BrhPageBase
    !insertmacro BrhPageBg

    ${NSD_CreateBitmap} 4% 26% 92% 70% ""
    Pop $hCard
    ${NSD_SetStretchedBitmap} $hCard "${UIDIR}\card.bmp" $hBmpCard

    System::Call 'user32::SetWindowPos(p$hBg, p1, i0,i0,i0,i0, i0x13)'
    System::Call 'user32::SetWindowPos(p$hCard, p0, i0,i0,i0,i0, i0x13)'
FunctionEnd

; Uninstaller counterparts: NSIS requires un.* functions to be called from the
; uninstaller, so the shared helpers are duplicated with that prefix.

Function un.BrhCreateFonts
    System::Call 'gdi32::CreateFont(i -17, i 0, i 0, i 0, i 700, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t "Segoe UI") p.r0'
    StrCpy $hFontTitle $0
    System::Call 'gdi32::CreateFont(i -13, i 0, i 0, i 0, i 400, i 0, i 0, i 0, i 1, i 0, i 0, i 5, i 0, t "Segoe UI") p.r0'
    StrCpy $hFontBody $0
FunctionEnd

Function un.BrhHideChrome
    StrCpy $R0 1028
    ${Do}
        GetDlgItem $R1 $HWNDPARENT $R0
        ${If} $R1 != 0
            ShowWindow $R1 0
        ${EndIf}
        IntOp $R0 $R0 + 1
    ${LoopUntil} $R0 > 1050

    GetDlgItem $R1 $HWNDPARENT 1256
    ${If} $R1 != 0
        ShowWindow $R1 0
    ${EndIf}
FunctionEnd

Function un.BrhFitPage
    System::Call 'user32::GetClientRect(p$HWNDPARENT, @r1)'
    System::Call '*$1(i,i,i.r2,i.r3)'
    StrCpy $PageW $2
    IntOp $PageH $3 - 42

    GetDlgItem $4 $HWNDPARENT 1
    ${If} $4 != 0
        System::Call 'user32::GetWindowRect(p$4, @r5)'
        System::Call 'user32::MapWindowPoints(p0, p$HWNDPARENT, pr5, i2)'
        System::Call '*$5(i,i.r6,i,i)'
        ${If} $6 > 80
        ${AndIf} $6 < $3
            IntOp $PageH $6 - 8
        ${EndIf}
    ${EndIf}

    ${If} $PageH < 100
        IntOp $PageH $3 - 42
    ${EndIf}

    System::Call 'user32::SetWindowPos(p$hPage, p0, i0, i0, i$PageW, i$PageH, i0x14)'
FunctionEnd

Function un.BrhPageBase
    !insertmacro BrhPageBg

    ${NSD_CreateBitmap} 4% 26% 92% 70% ""
    Pop $hCard
    ${NSD_SetStretchedBitmap} $hCard "${UIDIR}\card.bmp" $hBmpCard

    System::Call 'user32::SetWindowPos(p$hBg, p1, i0,i0,i0,i0, i0x13)'
    System::Call 'user32::SetWindowPos(p$hCard, p0, i0,i0,i0,i0, i0x13)'
FunctionEnd

!macro BrhBeginPage
    nsDialogs::Create 1018
    Pop $hPage
    ${If} $hPage == error
        Abort
    ${EndIf}
    Call BrhHideChrome
    Call BrhFitPage
    Call BrhPageBase
!macroend

!macro BrhBeginPageUn
    nsDialogs::Create 1018
    Pop $hPage
    ${If} $hPage == error
        Abort
    ${EndIf}
    Call un.BrhHideChrome
    Call un.BrhFitPage
    Call un.BrhPageBase
!macroend

!macro BrhTitle TEXT
    ${NSD_CreateLabel} 6% 30% 88% 12u "${TEXT}"
    Pop $hTitle
    SendMessage $hTitle ${WM_SETFONT} $hFontTitle 1
    SetCtlColors $hTitle "FFD79A" "transparent"
    System::Call 'user32::SetWindowPos(p$hTitle, p0, i0,i0,i0,i0, i0x13)'
!macroend

!macro BrhBody TEXT
    ${NSD_CreateLabel} 6% 38% 88% 54% "${TEXT}"
    Pop $hBody
    SendMessage $hBody ${WM_SETFONT} $hFontBody 1
    SetCtlColors $hBody "E8DCC3" "transparent"
    System::Call 'user32::SetWindowPos(p$hBody, p0, i0,i0,i0,i0, i0x13)'
!macroend

!macro BrhNoteControl VAR TEXT X Y W H
    ${NSD_CreateLabel} "${X}" "${Y}" "${W}" "${H}" "${TEXT}"
    Pop ${VAR}
    SendMessage ${VAR} ${WM_SETFONT} $hFontBody 1
    SetCtlColors ${VAR} "E8DCC3" "transparent"
    System::Call 'user32::SetWindowPos(p${VAR}, p0, i0,i0,i0,i0, i0x13)'
!macroend

; --- Disclaimer

Function BrhDisclaimerCreate
    !insertmacro BrhBeginPage
    !insertmacro BrhTitle "$(BrhDisclaimerTitle)"
    !insertmacro BrhBody "$(BrhDisclaimerText)"
    nsDialogs::Show
FunctionEnd

; --- Install folder

Function BrhBrowse
    nsDialogs::SelectFolderDialog "$(BrhSelectFolder)" "$INSTDIR"
    Pop $0
    ${If} $0 != "error"
    ${AndIf} $0 != ""
        ${NSD_SetText} $hEdit "$0"
    ${EndIf}
FunctionEnd

Function BrhDirectoryCreate
    !insertmacro BrhBeginPage
    !insertmacro BrhTitle "$(BrhDirTitle)"

    !insertmacro BrhNoteControl $hNote "$(BrhDirLabel)" "6%" "38%" "88%" "8u"

    ${NSD_CreateDirRequest} 6% 44% 66% 13u "$INSTDIR"
    Pop $hEdit
    SendMessage $hEdit ${WM_SETFONT} $hFontBody 1
    System::Call 'user32::SetWindowPos(p$hEdit, p0, i0,i0,i0,i0, i0x13)'

    ${NSD_CreateBrowseButton} 74% 44% 20% 13u "$(BrhBrowse)"
    Pop $hBrowse
    SendMessage $hBrowse ${WM_SETFONT} $hFontBody 1
    ${NSD_OnClick} $hBrowse BrhBrowse
    System::Call 'user32::SetWindowPos(p$hBrowse, p0, i0,i0,i0,i0, i0x13)'

    !insertmacro BrhNoteControl $hBody "$(BrhDirNote)" "6%" "52%" "88%" "40%"

    ; Upgrade hint: the text is replaced by the installer after the page exists.
    IfFileExists "$INSTDIR\game.exe" 0 brh_no_upgrade
    ${NSD_SetText} $hBody "$(BrhDirUpgrade)$\r$\n$\r$\n$(BrhDirNote)"
    brh_no_upgrade:

    nsDialogs::Show
FunctionEnd

Function BrhDirectoryLeave
    ${NSD_GetText} $hEdit $INSTDIR
    ${If} $INSTDIR == ""
        MessageBox MB_ICONEXCLAMATION "$(BrhDirEmpty)"
        Abort
    ${EndIf}
FunctionEnd

; --- Components / optional Start Menu shortcut

Function BrhComponentsCreate
    !insertmacro BrhBeginPage
    !insertmacro BrhTitle "$(BrhCompTitle)"

    ${NSD_CreateCheckBox} 6% 39% 60% 12u "$(BrhGameComp)"
    Pop $hChkGame
    ${NSD_Check} $hChkGame
    EnableWindow $hChkGame 0
    SendMessage $hChkGame ${WM_SETFONT} $hFontBody 1
    SetCtlColors $hChkGame "E8DCC3" "100C08"
    ; SetCtlColors does not reach the text of a themed check box (NSIS bug #443);
    ; dropping the theme on the control is MUI's own workaround and makes the
    ; owner's colors apply, so the label is readable on the dark card.
    System::Call 'UXTHEME::SetWindowTheme(p$hChkGame, w " ", w " ")'
    System::Call 'user32::SetWindowPos(p$hChkGame, p0, i0,i0,i0,i0, i0x13)'

    !insertmacro BrhNoteControl $hNote "$(BrhGameCompNote)" "8%" "45%" "86%" "22u"

    ; The two checkboxes get a full row between them: the upper note wraps to two
    ; lines in both languages, and at a larger DPI the old 57% park made the second
    ; box collide with it (the label looked attached to the note).
    ${NSD_CreateCheckBox} 6% 63% 88% 12u "$(BrhShortcut)"
    Pop $hChkShortcut
    SendMessage $hChkShortcut ${WM_SETFONT} $hFontBody 1
    SetCtlColors $hChkShortcut "E8DCC3" "100C08"
    System::Call 'UXTHEME::SetWindowTheme(p$hChkShortcut, w " ", w " ")'
    System::Call 'user32::SetWindowPos(p$hChkShortcut, p0, i0,i0,i0,i0, i0x13)'

    ; Preserve an existing shortcut across upgrades: show it as checked.
    IfFileExists "${SHORTCUT_LINK}" 0 brh_no_existing_shortcut
    ${NSD_Check} $hChkShortcut
    brh_no_existing_shortcut:

    !insertmacro BrhNoteControl $hBody "$(BrhShortcutNote)" "8%" "70%" "86%" "28u"

    nsDialogs::Show
FunctionEnd

Function BrhComponentsLeave
    ${NSD_GetState} $hChkShortcut $0
    ${If} $0 == ${BST_CHECKED}
        StrCpy $CreateShortcut "1"
    ${Else}
        StrCpy $CreateShortcut "0"
    ${EndIf}
FunctionEnd

; --- Install page: the "in progress" console art with the native progress bar in its slot

Function BrhInstfilesShow
    FindWindow $hPage "#32770" "" $HWNDPARENT
    Call BrhHideChrome
    Call BrhFitPage

    ; This page needs no click: it leaves by itself when the installation is really
    ; complete (SetAutoClose in .onInit). The Next button is therefore hidden -
    ; only the progress art and Cancel remain. BrhFinishCreate shows it again.
    GetDlgItem $R8 $HWNDPARENT 1
    ${If} $R8 != 0
        ShowWindow $R8 0
    ${EndIf}

    !insertmacro BrhPageBg

    ; The console panel from the owner's art (308x162) is centred above the
    ; status line; the progress bar sits inside its horizontal groove.
    IntOp $R0 $PageW - ${PANEL_W}
    IntOp $R0 $R0 / 2
    StrCpy $R1 80
    ${If} $PageH < 300
        StrCpy $R1 55
    ${EndIf}
    StrCpy $PanelY $R1

    System::Call 'user32::CreateWindowEx(i0, t "STATIC", t "", i0x5000000E, i$R0, i$R1, i${PANEL_W}, i${PANEL_H}, p$hPage, p0, p0, p0) p.r5'
    System::Call 'user32::LoadImage(p0, t "${UIDIR}\panel.bmp", i0, i0, i0, i0x10) p.r6'
    SendMessage $5 0x0172 0 $6
    StrCpy $hPanel $5
    StrCpy $hPanelBmp $6

    System::Call 'user32::SetWindowPos(p$hBg, p1, i0,i0,i0,i0, i0x13)'
    System::Call 'user32::SetWindowPos(p$hPanel, p$hBg, i0,i0,i0,i0, i0x13)'

    ; Live progress fill made from the owner's progress-bar art. The native
    ; NSIS bar is hidden; BrhProgress reveals this bitmap from the left. The
    ; control is created at the left edge of the groove and kept one pixel
    ; inside the slot opening on every side (see the SLOT_* defines).
    IntOp $R6 $R0 + ${SLOT_LEFT}
    IntOp $R7 $PanelY + ${SLOT_TOP}
    StrCpy $BarFillX $R6
    StrCpy $BarFillY $R7
    System::Call 'user32::CreateWindowEx(i0, t "STATIC", t "", i0x5000000E, i$R6, i$R7, i2, i${SLOT_H}, p$hPage, p0, p0, p0) p.r5'
    System::Call 'user32::LoadImage(p0, t "${UIDIR}\bar-fill.bmp", i0, i0, i0, i0x10) p.r6'
    SendMessage $5 0x0172 0 $6
    StrCpy $hBarFill $5
    StrCpy $hBarFillBmp $6
    ; Draw the fill directly above the panel art (the groove background), and
    ; keep it inside the slot opening so the painted rim stays on top visually.
    System::Call 'user32::SetWindowPos(p$hBarFill, p$hPanel, i0,i0,i0,i0, i0x13)'

    GetDlgItem $R2 $hPage 1004
    GetDlgItem $R3 $hPage 1006
    GetDlgItem $R4 $hPage 1016
    GetDlgItem $R5 $hPage 1027
    ShowWindow $R2 0
    ShowWindow $R3 0
    ShowWindow $R4 0
    ShowWindow $R5 0

    IntOp $R8 $PanelY + 172
    System::Call 'user32::CreateWindowEx(i0, t "STATIC", t "", i0x50000000, i24, i$R8, i$PageW, i40, p$hPage, p0, p0, p0) p.r9'
    StrCpy $hInstStatus $9
    SendMessage $hInstStatus ${WM_SETTEXT} 0 "STR:$(BrhInstalling)"
    SetCtlColors $hInstStatus "E8DCC3" "1E1912"
    SendMessage $hInstStatus ${WM_SETFONT} $hFontBody 1
    System::Call 'user32::SetWindowPos(p$hInstStatus, p0, i0,i0,i0,i0, i0x13)'

    System::Call 'user32::InvalidateRect(p$hPage, p0, i1)'
FunctionEnd

; Sets the custom progress fill (0..100) and updates the status line.
Function BrhProgress
    ${If} $hBarFill != 0
        IntOp $1 ${SLOT_W} * $0
        IntOp $1 $1 / 100
        ${If} $1 < 2
            StrCpy $1 2
        ${EndIf}
        ; Height stays at the slot height; only the left-to-right reveal grows.
        System::Call 'user32::SetWindowPos(p$hBarFill, p0, i$BarFillX, i$BarFillY, i$1, i${SLOT_H}, i0x14)'
        ${If} $hInstStatus != 0
            SendMessage $hInstStatus ${WM_SETTEXT} 0 "STR:$(BrhInstalling) $0%"
        ${EndIf}
        System::Call 'user32::InvalidateRect(p$hBarFill, p0, i1)'
        System::Call 'user32::UpdateWindow(p$hBarFill)'
    ${EndIf}
FunctionEnd

; --- Finish

; Measures the pixel height of the text in control $0 wrapped to that control's
; width with the body font. Result in $2 (0 when it cannot be measured).
; The finish page places the launch row from the real text height: the wrapped
; height differs between RU and EN and grows with the install path length, so a
; fixed percentage either glued the checkbox to the paragraph above it or pushed
; it out of the content card.
Function BrhTextHeight
    StrCpy $2 0
    ${If} $0 == 0
    ${OrIf} $hFontBody == 0
        Return
    ${EndIf}

    System::Call 'user32::GetClientRect(p$0, @r3)'
    System::Call '*$3(i, i, i.r4, i)'
    ${If} $4 <= 0
        Return
    ${EndIf}

    ; DrawText with DT_CALCRECT | DT_WORDBREAK follows the same wrapping rules as
    ; the static control, so the result is the height the label will really use.
    System::Call 'user32::GetDC(p0) p.r5'
    System::Call 'gdi32::SelectObject(p$5, p$hFontBody) p.r6'
    System::Call '*(i, i, i, i) p.r7'
    System::Call '*$7(i 0, i 0, i $4, i 0)'
    System::Call 'user32::DrawTextW(p$5, t r1, i -1, p$7, i 0x0410|0x2000) i.r8'
    System::Call '*$7(i, i, i, i.r2)'
    System::Call 'gdi32::SelectObject(p$5, p$6) p.r9'
    System::Call 'user32::ReleaseDC(p0, p$5)'
FunctionEnd

Function BrhFinishCreate
    !insertmacro BrhBeginPage
    !insertmacro BrhTitle "$(BrhFinishTitle)"

    ${NSD_CreateBitmap} 6% 36% 88% 21% ""
    Pop $hBg
    ${NSD_SetStretchedBitmap} $hBg "${UIDIR}\banner-strip.bmp" $hBmpExtra
    System::Call 'user32::SetWindowPos(p$hBg, p0, i0,i0,i0,i0, i0x13)'

    !insertmacro BrhNoteControl $hBody "$(BrhFinishText)" "6%" "58%" "88%" "10%"

    StrCpy $0 $hBody
    StrCpy $1 "$(BrhFinishText)"
    Call BrhTextHeight
    ${If} $2 < 17
        StrCpy $2 68
    ${EndIf}

    ; The launch row is created early so its own height (which depends on the DPI)
    ; is known; the layout is then solved from the bottom edge of the content card:
    ; the row is pinned there, the text block sits above it with a fixed gap, and
    ; the banner takes the rest. A long install path wraps the text to more lines
    ; and squeezes the banner instead of pushing the row over the text.
    ${NSD_CreateCheckBox} "6%" "58%" "88%" "12u" "$(BrhRunGame)"
    Pop $hRunGame
    ${NSD_Check} $hRunGame
    SendMessage $hRunGame ${WM_SETFONT} $hFontBody 1
    SetCtlColors $hRunGame "E8DCC3" "100C08"
    System::Call 'UXTHEME::SetWindowTheme(p$hRunGame, w " ", w " ")'

    System::Call 'user32::GetWindowRect(p$hBody, @r3)'
    System::Call 'user32::MapWindowPoints(p0, p$hPage, pr3, i2)'
    System::Call '*$3(i.r4, i.r5, i.r6, i.r7)'

    System::Call 'user32::GetWindowRect(p$hRunGame, @r3)'
    System::Call 'user32::MapWindowPoints(p0, p$hPage, pr3, i2)'
    System::Call '*$3(i, i.r8, i, i.r9)'

    IntOp $R0 $PageH * 96
    IntOp $R0 $R0 / 100          ; content card bottom
    IntOp $8 $9 - $8             ; launch row height
    IntOp $9 $R0 - $8
    IntOp $9 $9 - 8              ; row top, pinned to the card bottom
    IntOp $R1 $9 - 16            ; text bottom with a clear gap above the row
    IntOp $R1 $R1 - $2           ; text top

    ; Text block: exact left edge and width, measured height plus one pixel.
    IntOp $6 $6 - $4
    IntOp $7 $2 + 2
    System::Call 'user32::SetWindowPos(p$hBody, p0, i$4, i$R1, i$6, i$7, i0x14)'

    ; Banner between the title and the text; hidden rather than squashed to a
    ; sliver when the text needs the room.
    IntOp $R2 $R1 - 10
    IntOp $R3 $PageH * 36
    IntOp $R3 $R3 / 100
    IntOp $R4 $R2 - $R3
    ${If} $R4 < 40
        ShowWindow $hBg 0
    ${Else}
        System::Call 'user32::SetWindowPos(p$hBg, p0, i$4, i$R3, i$6, i$R4, i0x14)'
    ${EndIf}

    ; Launch row: the box sits on the text margin and its label reads to the right
    ; of the box, on a line of its own. HWND_TOP keeps the row above the content
    ; card; a plain move (SWP_NOZORDER) can leave it behind the card.
    System::Call 'user32::SetWindowPos(p$hRunGame, p0, i$4, i$9, i$6, i$8, i0x10)'
    System::Call 'user32::InvalidateRect(p$hPage, p0, i1)'

    ; The install page hid the Next button; this page needs it as Close/Finish.
    GetDlgItem $R0 $HWNDPARENT 1
    ${If} $R0 != 0
        ShowWindow $R0 1
    ${EndIf}

    nsDialogs::Show
FunctionEnd

Function BrhFinishLeave
    ${NSD_GetState} $hRunGame $0
    ${If} $0 == ${BST_CHECKED}
        ${If} ${FileExists} "$INSTDIR\game.exe"
            Exec '"$INSTDIR\game.exe"'
        ${EndIf}
    ${EndIf}
FunctionEnd

; --- Uninstaller pages

Function un.BrhWelcomeCreate
    !insertmacro BrhBeginPageUn
    !insertmacro BrhTitle "$(BrhUninstTitle)"
    !insertmacro BrhBody "$(BrhUninstText)"
    nsDialogs::Show
FunctionEnd

Function un.BrhOptionsCreate
    !insertmacro BrhBeginPageUn
    !insertmacro BrhTitle "$(BrhUninstTitle)"

    ${NSD_CreateCheckBox} 6% 42% 88% 12u "$(BrhDeleteSaves)"
    Pop $hChkShortcut
    SendMessage $hChkShortcut ${WM_SETFONT} $hFontBody 1
    SetCtlColors $hChkShortcut "E8DCC3" "100C08"
    System::Call 'UXTHEME::SetWindowTheme(p$hChkShortcut, w " ", w " ")'
    System::Call 'user32::SetWindowPos(p$hChkShortcut, p0, i0,i0,i0,i0, i0x13)'

    !insertmacro BrhNoteControl $hBody "$(BrhDeleteSavesNote)" "8%" "50%" "86%" "30%"

    nsDialogs::Show
FunctionEnd

Function un.BrhOptionsLeave
    ${NSD_GetState} $hChkShortcut $0
    ${If} $0 == ${BST_CHECKED}
        StrCpy $DeleteSaves "1"
    ${Else}
        StrCpy $DeleteSaves "0"
    ${EndIf}
FunctionEnd

Function un.BrhUninstfilesShow
    FindWindow $hPage "#32770" "" $HWNDPARENT
    Call un.BrhHideChrome
    Call un.BrhFitPage

    !insertmacro BrhPageBg

    IntOp $R0 $PageW - ${PANEL_W}
    IntOp $R0 $R0 / 2
    StrCpy $R1 80
    ${If} $PageH < 300
        StrCpy $R1 55
    ${EndIf}
    StrCpy $PanelY $R1

    System::Call 'user32::CreateWindowEx(i0, t "STATIC", t "", i0x5000000E, i$R0, i$R1, i${PANEL_W}, i${PANEL_H}, p$hPage, p0, p0, p0) p.r5'
    System::Call 'user32::LoadImage(p0, t "${UIDIR}\panel.bmp", i0, i0, i0, i0x10) p.r6'
    SendMessage $5 0x0172 0 $6
    StrCpy $hPanel $5
    StrCpy $hPanelBmp $6

    System::Call 'user32::SetWindowPos(p$hBg, p1, i0,i0,i0,i0, i0x13)'
    System::Call 'user32::SetWindowPos(p$hPanel, p$hBg, i0,i0,i0,i0, i0x13)'

    GetDlgItem $R2 $hPage 1004
    GetDlgItem $R3 $hPage 1006
    GetDlgItem $R4 $hPage 1016
    GetDlgItem $R5 $hPage 1027
    ShowWindow $R3 0
    ShowWindow $R4 0
    ShowWindow $R5 0

    ; The uninstaller keeps the native NSIS progress bar; it is placed in the same
    ; groove rectangle as the custom fill of the install page.
    IntOp $R6 $R0 + ${SLOT_LEFT}
    IntOp $R7 $PanelY + ${SLOT_TOP}
    System::Call 'user32::SetWindowPos(p$R2, p0, i$R6, i$R7, i${SLOT_W}, i${SLOT_H}, i0x14)'
    System::Call 'user32::SetWindowPos(p$R2, p0, i0,i0,i0,i0, i0x13)'

    IntOp $R8 $PanelY + 172
    StrCpy $R9 "$(BrhRemoving)"
    System::Call 'user32::CreateWindowEx(i0, t "STATIC", t "$R9", i0x50000000, i24, i$R8, i$PageW, i40, p$hPage, p0, p0, p0) p.r9'
    StrCpy $hInstStatus $9
    SetCtlColors $hInstStatus "E8DCC3" "1E1912"
    SendMessage $hInstStatus ${WM_SETFONT} $hFontBody 1
    System::Call 'user32::SetWindowPos(p$hInstStatus, p0, i0,i0,i0,i0, i0x13)'

    System::Call 'user32::InvalidateRect(p$hPage, p0, i1)'
FunctionEnd

Function un.BrhFinishCreate
    !insertmacro BrhBeginPageUn
    !insertmacro BrhTitle "$(BrhUninstDoneTitle)"

    !insertmacro BrhNoteControl $hBody "$(BrhUninstDoneText)" "6%" "40%" "88%" "40%"
    ${If} $DeleteSaves == "1"
        ${NSD_SetText} $hBody "$(BrhUninstDoneTextDeleted)"
    ${EndIf}

    nsDialogs::Show
FunctionEnd

; --- Sections

Section "$(SecGame)" SecGame
    SectionIn RO

    SetOutPath "$INSTDIR"

    ; Pristine folders the game writes into; created before the payload, so an
    ; upgrade keeps whatever is already there.
    CreateDirectory "$INSTDIR\saves"
    CreateDirectory "$INSTDIR\missions"

    ; Payload and progress. scripts/build-installer.ps1 generates both: every
    ; group of File commands is followed by the percentage of payload bytes copied
    ; so far, so the bar follows the real copying instead of an arbitrary scale.
    ; It stops at 99% here - 100% is only reported once the installation has
    ; really finished (see the end of this section).
    !ifdef DATAGROUPS
        !include "${DATAGROUPS}"
    !else
        File "${STAGE}\game.exe"
        File "${STAGE}\SDL3.dll"
        File "${STAGE}\LICENSE"
        File "${STAGE}\THIRD_PARTY_LICENSES.txt"
        File "${STAGE}\README.txt"
        File "${STAGE}\CHANGELOG.md"

        ; The template is installed only when the player does not have settings
        ; yet, so an upgrade never overwrites settings.
        IfFileExists "$INSTDIR\settings.jsonc" brh_keep_settings
        File "${STAGE}\settings.jsonc"
        brh_keep_settings:

        SetOutPath "$INSTDIR\data"
        File /r "${STAGE}\data\*.*"
        StrCpy $0 60
        Call BrhProgress

        SetOutPath "$INSTDIR\remaster"
        File /r "${STAGE}\remaster\*.*"
        StrCpy $0 99
        Call BrhProgress
    !endif

    SetOutPath "$INSTDIR"
    WriteUninstaller "$INSTDIR\uninstall.exe"

    ; Optional Start Menu shortcut: only when the checkbox (or /SHORTCUT) was set.
    ${If} $CreateShortcut == "1"
        CreateDirectory "${SHORTCUT_FOLDER}"
        CreateShortcut "${SHORTCUT_LINK}" "$INSTDIR\game.exe" "" "$INSTDIR\game.exe" 0
    ${EndIf}

    ; Real completion: every payload byte is on disk, the uninstaller is written
    ; and the optional shortcut exists. Only now the bar reaches 100%; the short
    ; pause lets the final frame paint, then SetAutoClose (see .onInit) moves the
    ; wizard to the finish page on its own - the user never presses Next.
    StrCpy $0 100
    Call BrhProgress
    Sleep 900
SectionEnd

Section "Uninstall"
    ; Remove the optional Start Menu shortcut. Nothing else outside $INSTDIR was
    ; ever created, so nothing else is touched.
    Delete "${SHORTCUT_LINK}"
    RMDir "${SHORTCUT_FOLDER}"

    ; Game files and data (these are ours; player data is handled separately).
    Delete "$INSTDIR\game.exe"
    Delete "$INSTDIR\SDL3.dll"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\THIRD_PARTY_LICENSES.txt"
    Delete "$INSTDIR\README.txt"
    Delete "$INSTDIR\CHANGELOG.md"
    RMDir /r "$INSTDIR\data"
    RMDir /r "$INSTDIR\remaster"

    ; Configuration files belong to the installation and are removed with it.
    Delete "$INSTDIR\settings.jsonc"
    Delete "$INSTDIR\preferences.ini"

    ; Saves (and campaign progress) are kept unless the player ticked the box
    ; (or passed /SAVES).
    ${If} $DeleteSaves == "1"
        RMDir /r "$INSTDIR\saves"
    ${EndIf}
    RMDir "$INSTDIR\saves"
    RMDir "$INSTDIR\missions"

    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"
SectionEnd
