#!/bin/sh
# Inventário do que o Playwright (Python) e o pyppeteer exigem do sistema, medido dentro do oráculo de automação.
D=$(/opt/playwright/bin/python -c "import playwright,os;print(os.path.dirname(playwright.__file__))")
echo "== playwright pip"
/opt/playwright/bin/pip list 2>/dev/null
echo "== driver"
ls -la "$D/driver"
file "$D/driver/node" | cut -c1-200
"$D/driver/node" --version
echo "== ldd do node do driver"
ldd "$D/driver/node"
echo "== imports de greenlet na wheel"
grep -rn "greenlet" "$D" --include=*.py | head -20
echo "== extensões nativas nos pacotes do venv"
find /opt/playwright/lib -name '*.so*' | head -20
echo "== pyppeteer pip"
/opt/pyppeteer/bin/pip list 2>/dev/null
echo "== extensões nativas no venv do pyppeteer"
find /opt/pyppeteer/lib -name '*.so*' | head -20
