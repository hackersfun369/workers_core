@echo off
REM deploy-worker-core.bat
REM Deploys worker-core and all dependent workers to Cloudflare (Windows)
REM Usage: deploy-worker-core.bat [environment]

setlocal enabledelayedexpansion

set ENVIRONMENT=%~1
if "%ENVIRONMENT%"=="" set ENVIRONMENT=production

echo === Autonomous Software Factory - Deploy Script (Windows) ===
echo Environment: %ENVIRONMENT%
echo.

REM Check prerequisites
echo Checking prerequisites...

where wrangler >nul 2>nul
if %errorlevel% neq 0 (
    echo ERROR: wrangler CLI not found. Install with: npm install -g wrangler
    exit /b 1
)

where cargo >nul 2>nul
if %errorlevel% neq 0 (
    echo ERROR: cargo not found. Install Rust from https://rustup.rs/
    exit /b 1
)

echo ✓ Prerequisites met
echo.

REM Build WASM
echo Building worker-core for WASM target...
wasm-pack build --target web --release
if %errorlevel% neq 0 (
    echo ERROR: WASM build failed
    exit /b 1
)
echo ✓ WASM build complete
echo.

REM Deploy worker
echo Deploying worker-core...
wrangler deploy
if %errorlevel% neq 0 (
    echo ERROR: Deployment failed
    exit /b 1
)
echo ✓ worker-core deployed
echo.

echo === Deployment Complete ===
echo.
