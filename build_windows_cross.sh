#!/bin/bash
# macOS에서 Windows .exe 크로스 컴파일 시도
# 주의: eframe(GUI)은 크로스 컴파일이 까다로워 실패할 수 있습니다.
#       실패 시 GitHub Actions 또는 Windows 직접 빌드를 사용하세요.

set -e

echo "=== Windows 크로스 컴파일 시도 ==="
echo

# 1. mingw-w64 설치 확인
if ! command -v x86_64-w64-mingw32-gcc &> /dev/null; then
    echo "[1/3] mingw-w64 설치 중..."
    brew install mingw-w64
else
    echo "[1/3] mingw-w64 이미 설치됨 ✓"
fi

# 2. Windows Rust 타겟 추가
echo "[2/3] Windows 타겟 추가 중..."
rustup target add x86_64-pc-windows-gnu

# 3. .cargo/config.toml 설정 (linker 지정)
mkdir -p .cargo
cat > .cargo/config.toml <<EOF
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
ar = "x86_64-w64-mingw32-ar"
EOF

# 4. 빌드 시도
echo "[3/3] Windows .exe 빌드 중... (실패 시 정상)"
echo
if cargo build --release --target x86_64-pc-windows-gnu 2>&1; then
    echo
    echo "✅ 성공!"
    echo "결과: target/x86_64-pc-windows-gnu/release/rust_test.exe"
    ls -lh target/x86_64-pc-windows-gnu/release/rust_test.exe
else
    echo
    echo "❌ 실패. eframe(GUI)는 macOS에서 Windows로 크로스 컴파일이 어렵습니다."
    echo
    echo "👉 대안:"
    echo "   1. GitHub Actions 사용 (.github/workflows/release.yml 이미 설정됨)"
    echo "   2. Windows PC/VM에서 직접 빌드"
    echo "   3. Parallels/VMware Fusion 으로 Windows 가상머신 설치"
fi
