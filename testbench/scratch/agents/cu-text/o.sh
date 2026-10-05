#!/bin/bash
# uso: o.sh 'comando bash'   (roda no oráculo num tmpfs)
docker run --rm -i --tmpfs /work:exec pseudo-linus-oracle:719900900623 bash -c "cd /work && $1"
