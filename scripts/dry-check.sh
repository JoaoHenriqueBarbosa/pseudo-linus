#!/usr/bin/env bash
#
# Lint de DRY: funções parecidas (similarity-rs) e funções de repasse (scripts/dry-forwarders.py).
#
# Uso:
#   scripts/dry-check.sh              checa os .rs do índice do git (o que o pre-commit roda)
#   scripts/dry-check.sh ARQ...       checa esses arquivos
#   scripts/dry-check.sh --rebuild    refaz scripts/dry-baseline.txt varrendo o repositório inteiro
#
# Cada arquivo checado é comparado com os demais arquivos do mesmo crate. Um par acima do limiar ou
# um repasse que toque um arquivo checado e que não esteja em scripts/dry-baseline.txt recusa a
# checagem. A lista é a dívida que já existia quando a régua chegou: ela só encolhe. Entrada que
# deixou de acontecer é avisada, para sair da lista no mesmo commit que a resolveu.
#
# O limiar padrão (0,90) é mais frouxo que o do claudia (0,20) porque, a 0,20, este repositório
# tem mais de 255 mil pares e a varredura completa leva minutos.

set -o pipefail

root=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
cd "$root" || exit 1

threshold="${DRY_THRESHOLD:-0.90}"
min_lines="${DRY_MIN_LINES:-3}"
baseline=scripts/dry-baseline.txt

# Código de terceiros, testes e o que não é do projeto ficam fora.
own_rs() {
	grep -E '^crates/.*\.rs$' | grep -vE '/(vendor|staging|tests|benches)/|/tests\.rs$'
}

# Pares do similarity-rs como `arquivo função <-> arquivo função`, sem linhas e em ordem estável.
pairs_of() {
	similarity-rs "$@" --min-lines "$min_lines" --threshold "$threshold" --skip-test 2>/dev/null |
		awk '
			/ <-> / {
				line = $0
				sub(/^[ \t]+/, "", line)
				split(line, sides, / <-> /)
				for (k = 1; k <= 2; k++) {
					s = sides[k]
					sub(/:[0-9]+-[0-9]+ function /, " ", s)
					gsub(/^[ \t]+|[ \t]+$/, "", s)
					sides[k] = s
				}
				if (sides[1] > sides[2]) { t = sides[1]; sides[1] = sides[2]; sides[2] = t }
				if (sides[1] != sides[2]) print "pair " sides[1] " <-> " sides[2]
			}
		'
}

# Repasses como `forward arquivo função -> alvo`, sem a linha.
forwarders_of() {
	python3 scripts/dry-forwarders.py "$@" | sed -E 's/^([^:]+):[0-9]+ /forward \1 /'
}

if ! command -v similarity-rs &>/dev/null; then
	echo 'Aviso: similarity-rs não instalado, checagem de DRY pulada. Instale com: cargo install similarity-rs' >&2
	exit 0
fi

if [[ "${1:-}" == '--rebuild' ]]; then
	mapfile -t all < <(git ls-files 'crates/*.rs' | own_rs)
	{
		pairs_of "${all[@]}"
		forwarders_of crates
	} | sort -u >"$baseline"
	echo "$(wc -l <"$baseline") entrada(s) em $baseline."
	exit 0
fi

if [[ $# -gt 0 ]]; then
	mapfile -t checked < <(printf '%s\n' "$@" | own_rs)
else
	mapfile -t checked < <(git diff --cached --name-only --diff-filter=ACMR | own_rs)
fi
[[ ${#checked[@]} -eq 0 ]] && exit 0

# O crate de cada arquivo checado: o par pode estar em qualquer arquivo dele.
mapfile -t crates < <(printf '%s\n' "${checked[@]}" | cut -d/ -f1-2 | sort -u)
mapfile -t scope < <(git ls-files "${crates[@]/%//*.rs}" | own_rs)

touches_checked() {
	local line
	while IFS= read -r line; do
		for f in "${checked[@]}"; do
			if [[ "$line" == *" $f "* ]]; then
				printf '%s\n' "$line"
				break
			fi
		done
	done
}

found=$({
	pairs_of "${scope[@]}"
	forwarders_of "${checked[@]}"
} | touches_checked | sort -u)

known=$(grep -F -f <(printf '%s\n' "${checked[@]/#/ }" | sed 's/$/ /') "$baseline" 2>/dev/null | sort -u)
new=$(comm -23 <(printf '%s\n' "$found" | sed '/^$/d') <(printf '%s\n' "$known" | sed '/^$/d'))
gone=$(comm -13 <(printf '%s\n' "$found" | sed '/^$/d') <(printf '%s\n' "$known" | sed '/^$/d'))

if [[ -n "$gone" ]]; then
	{
		echo "Dívida de DRY resolvida (tire de $baseline neste commit):"
		printf '%s\n' "$gone" | sed 's/^/  /'
		echo
	} >&2
fi

[[ -z "$new" ]] && exit 0

{
	echo "Duplicação nova: $(printf '%s\n' "$new" | wc -l) achado(s) acima de ${threshold} ou repasse de uma linha."
	echo
	printf '%s\n' "$new" | sed 's/^/  /'
	echo
	echo 'Os caminhos que funcionam, em ordem de preferência:'
	echo
	echo '  1. dar nome ao conceito repetido e extrair a função que faltava;'
	echo '  2. fundir as duas numa só, com a diferença virando parâmetro;'
	echo '  3. escrever a macro que gera o que se repete;'
	echo '  4. mudar o tipo para que a repetição deixe de ser possível.'
	echo
	echo 'Repasse (função cujo corpo é só a chamada de outra com os mesmos argumentos) sai:'
	echo 'o chamador chama o alvo direto, ou o nome vira reexportação (pub use x::y as z).'
	echo
	echo "Pares: similarity-rs ARQUIVOS --min-lines ${min_lines} --threshold ${threshold} --skip-test --print"
} >&2

exit 1
