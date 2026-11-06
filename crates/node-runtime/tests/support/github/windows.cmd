@echo off
setlocal
>>"%HOME%\gh-calls" echo %*
if not defined LOGIN_SECRET set "LOGIN_SECRET=unset"
if not defined SAILRY_MODEL_API_KEY set "SAILRY_MODEL_API_KEY=unset"
>"%HOME%\gh-environment" echo %HOME%:%LOGIN_SECRET%:%SAILRY_MODEL_API_KEY%
if "%*"=="auth status --active --hostname github.com --json hosts" goto status
if "%*"=="auth token --hostname github.com --user fixture-user" goto token
if "%*"=="auth token --hostname github.com --user other-user" goto token
exit /b 2

:status
>&2 echo synthetic-gh-private-token
if exist "%HOME%\gh-signed-out" goto signed_out
set "login=fixture-user"
if exist "%HOME%\gh-switched" set "login=other-user"
echo {"hosts":{"github.com":[{"active":true,"host":"github.com","state":"success","login":"%login%","tokenSource":"keyring"}]}}
exit /b 0

:signed_out
echo {"hosts":{}}
exit /b 0

:token
if exist "%HOME%\gh-fail" goto failed
if not exist "%HOME%\gh-hold" goto ready
type nul >"%HOME%\gh-entered"
:waiting
if not exist "%HOME%\gh-release" goto waiting

:ready
if exist "%HOME%\gh-oversized" goto oversized
if exist "%HOME%\gh-rotated" (
    echo synthetic-gh-new-token
) else (
    echo synthetic-gh-private-token
)
exit /b 0

:failed
>&2 echo synthetic-gh-private-token
exit /b 1

:oversized
for /l %%i in (1,1,5000) do <nul set /p "=x"
exit /b 0
