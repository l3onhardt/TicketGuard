@echo off
rem Sandbox cargo: bundled toolchain + vendored crates, fully offline.
rem Usage: tools\codex\cargo.cmd test
rem This file must stay ASCII-only (cmd.exe reads it in the OEM code page).
set "TG_ROOT=%~dp0..\.."
set "PATH=%TG_ROOT%\.toolchain\rust\bin;%PATH%"
set "CARGO_HOME=%TG_ROOT%\.toolchain\cargo-home"
set "RUSTUP_HOME=%TG_ROOT%\.toolchain\rust"
set "CARGO_NET_OFFLINE=true"
set "CARGO_TARGET_DIR=%TG_ROOT%\.toolchain\target"
"%TG_ROOT%\.toolchain\rust\bin\cargo.exe" %*
