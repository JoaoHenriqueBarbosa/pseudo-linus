#!/bin/bash
# gera dados do oráculo para ajustar o modelo do ptx: um bloco por configuração
t1="alpha beta gamma delta epsilon zeta eta theta iota kappa"
t2="the quick brown fox jumps over the lazy dog and keeps running far away from here"
t3="a bb ccc dddd eeeee ffffff ggggggg hhhhhhhh iiiiiiiii jjjjjjjjjj"
n=0
for t in "$t1" "$t2" "$t3"; do
  for w in 20 30 40 50 72; do
    for g in 1 3 5; do
      for f in "/" ">>>" ""; do
        printf '%s\n' "$t" > in.txt
        echo "@@ w=$w g=$g f=[$f] t=[$t] opt=none"
        ptx -w $w -g $g -F "$f" in.txt
        echo "@@ w=$w g=$g f=[$f] t=[$t] opt=R"
        ptx -A -R -w $w -g $g -F "$f" in.txt
        echo "@@ w=$w g=$g f=[$f] t=[$t] opt=A"
        ptx -A -w $w -g $g -F "$f" in.txt
      done
    done
  done
done
