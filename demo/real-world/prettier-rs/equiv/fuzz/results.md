# Differential fuzzing results

Run 1: port before the fixes. Run 2: after round 1. Run 3: after round 2. N = inputs. bad = mismatch / soundness / panic-or-hang. 3/N = 95% upper bound on the mismatch rate over inputs both sides formatted, for cells with nothing bad. `·` = stream not run.

| gen | lang | N 1 | bad 1 | 3/N 1 | N 2 | bad 2 | 3/N 2 | N 3 | bad 3 | 3/N 3 |
|---|---|---|---|---|---|---|---|---|---|---|
| a splice-css | css | 104599 | 4 / 0 / 0 | - | 104599 | 0 / 0 / 0 | 2.9e-5 | 104599 | 0 / 0 / 0 | 2.9e-5 |
| a splice-css | less | 83867 | 2 / 0 / 0 | - | 83867 | 0 / 0 / 0 | 3.6e-5 | 83867 | 0 / 0 / 0 | 3.6e-5 |
| a splice-css | scss | 121534 | 3 / 0 / 0 | - | 121534 | 0 / 0 / 0 | 2.5e-5 | 121534 | 0 / 0 / 0 | 2.5e-5 |
| a splice-js | js | 245450 | 703 / 645 / 0 | - | 245450 | 7 / 0 / 0 | - | 245450 | 0 / 0 / 0 | 1.3e-5 |
| a splice-js | ts | 154550 | 51 / 197 / 0 | - | 154550 | 4 / 23 / 0 | - | 154550 | 0 / 6 / 0 | - |
| a2 graphql-tokens | graphql | 150000 | 0 / 0 / 0 | 7.6e-5 | 150000 | 0 / 0 / 0 | 7.6e-5 | 150000 | 0 / 0 / 0 | 7.6e-5 |
| b options(corpus) | css | 22158 | 0 / 0 / 0 | 1.4e-4 | 22158 | 0 / 0 / 0 | 1.4e-4 | 22158 | 0 / 0 / 0 | 1.4e-4 |
| b options(corpus) | graphql | 21300 | 0 / 0 / 0 | 1.5e-4 | 21300 | 0 / 0 / 0 | 1.5e-4 | 21300 | 0 / 0 / 0 | 1.5e-4 |
| b options(corpus) | js | 47800 | 1 / 0 / 17 | - | 47800 | 0 / 0 / 0 | 7.6e-5 | 47800 | 0 / 0 / 0 | 7.6e-5 |
| b options(corpus) | json | 36950 | 0 / 0 / 0 | 1.2e-4 | 36950 | 0 / 0 / 0 | 1.2e-4 | 36950 | 0 / 0 / 0 | 1.2e-4 |
| b options(corpus) | less | 11451 | 0 / 0 / 0 | 2.8e-4 | 11451 | 0 / 0 / 0 | 2.8e-4 | 11451 | 0 / 0 / 0 | 2.8e-4 |
| b options(corpus) | markdown | 158700 | 7 / 0 / 0 | - | 158700 | 0 / 0 / 0 | 1.9e-5 | 158700 | 0 / 0 / 0 | 1.9e-5 |
| b options(corpus) | other | 175 | 0 / 0 / 0 | 3.0e-2 | 175 | 0 / 0 / 0 | 3.0e-2 | 175 | 0 / 0 / 0 | 3.0e-2 |
| b options(corpus) | scss | 17941 | 0 / 0 / 0 | 1.7e-4 | 17941 | 0 / 0 / 0 | 1.7e-4 | 17941 | 0 / 0 / 0 | 1.7e-4 |
| b options(corpus) | ts | 23375 | 220 / 0 / 0 | - | 23375 | 0 / 0 / 0 | 1.4e-4 | 23375 | 0 / 0 / 0 | 1.4e-4 |
| b options(corpus) | yaml | 83525 | 38 / 0 / 0 | - | 83525 | 0 / 0 / 0 | 3.6e-5 | 83525 | 0 / 0 / 0 | 3.6e-5 |
| b options(splice-css) | css | 33475 | 1 / 0 / 0 | - | 33475 | 0 / 0 / 0 | 9.1e-5 | 33475 | 0 / 0 / 0 | 9.1e-5 |
| b options(splice-css) | less | 27648 | 0 / 0 / 0 | 1.1e-4 | 27648 | 0 / 0 / 0 | 1.1e-4 | 27648 | 0 / 0 / 0 | 1.1e-4 |
| b options(splice-css) | scss | 38877 | 1 / 0 / 0 | - | 38877 | 0 / 0 / 0 | 7.7e-5 | 38877 | 0 / 0 / 0 | 7.7e-5 |
| b options(splice-js) | js | 92175 | 272 / 305 / 106 | - | 92175 | 1 / 0 / 0 | - | 92175 | 0 / 0 / 0 | 3.6e-5 |
| b options(splice-js) | ts | 57825 | 624 / 73 / 0 | - | 57825 | 0 / 15 / 0 | - | 57825 | 0 / 0 / 0 | 5.3e-5 |
| c feedback (prettier(x) as input) | css | 25351 | 113 / 1 / 2 | - | 25263 | 0 / 0 / 0 | 1.2e-4 | 25263 | 0 / 0 / 0 | 1.2e-4 |
| c feedback (prettier(x) as input) | graphql | 43419 | 0 / 0 / 0 | 7.0e-5 | 43393 | 0 / 0 / 0 | 7.0e-5 | 43393 | 0 / 0 / 0 | 7.0e-5 |
| c feedback (prettier(x) as input) | js | 69145 | 87 / 0 / 0 | - | 69021 | 0 / 0 / 0 | 4.7e-5 | 69021 | 0 / 0 / 0 | 4.7e-5 |
| c feedback (prettier(x) as input) | json | 5087 | 0 / 0 / 0 | 6.1e-4 | 5089 | 0 / 0 / 0 | 6.1e-4 | 5089 | 0 / 0 / 0 | 6.1e-4 |
| c feedback (prettier(x) as input) | less | 19169 | 5 / 0 / 1 | - | 18582 | 1 / 0 / 0 | - | 18582 | 0 / 0 / 0 | 1.6e-4 |
| c feedback (prettier(x) as input) | markdown | 35569 | 3 / 0 / 0 | - | 30611 | 0 / 0 / 0 | 1.0e-4 | 30611 | 0 / 0 / 0 | 1.0e-4 |
| c feedback (prettier(x) as input) | other | 16 | 0 / 0 / 0 | 3.0e-1 | 16 | 0 / 0 / 0 | 3.0e-1 | 16 | 0 / 0 / 0 | 3.0e-1 |
| c feedback (prettier(x) as input) | scss | 28349 | 29 / 0 / 2 | - | 27285 | 2 / 0 / 0 | - | 27285 | 0 / 0 / 0 | 1.1e-4 |
| c feedback (prettier(x) as input) | ts | 41894 | 54 / 7 / 0 | - | 41577 | 1 / 2 / 0 | - | 41577 | 0 / 0 / 0 | 7.4e-5 |
| c feedback (prettier(x) as input) | yaml | 15792 | 0 / 0 / 0 | 1.9e-4 | 15523 | 0 / 0 / 0 | 2.0e-4 | 15523 | 0 / 0 / 0 | 2.0e-4 |
| d libfuzzer | css | 1375 | 117 / 0 / 4 | - | 1173 | 0 / 0 / 0 | 3.2e-3 | 1173 | 0 / 0 / 0 | 3.2e-3 |
| d libfuzzer | graphql | 591 | 0 / 0 / 0 | 7.9e-2 | 274 | 0 / 0 / 0 | 2.5e-1 | 274 | 0 / 0 / 0 | 2.5e-1 |
| d libfuzzer | js | 3154 | 0 / 13 / 0 | - | 2826 | 0 / 0 / 0 | 3.7e-3 | 2826 | 0 / 0 / 0 | 3.7e-3 |
| d libfuzzer | json | 476 | 0 / 0 / 0 | 6.7e-2 | 371 | 0 / 0 / 0 | 5.9e-2 | 371 | 0 / 0 / 0 | 5.9e-2 |
| d libfuzzer | less | 2558 | 7 / 0 / 1 | - | 1541 | 1 / 0 / 0 | - | 1541 | 0 / 0 / 0 | 2.9e-3 |
| d libfuzzer | markdown | 10394 | 7 / 0 / 0 | - | 5321 | 0 / 0 / 0 | 7.1e-4 | 5321 | 0 / 0 / 0 | 7.1e-4 |
| d libfuzzer | scss | 2898 | 59 / 1 / 3 | - | 1445 | 0 / 0 / 0 | 3.1e-3 | 1445 | 0 / 0 / 0 | 3.1e-3 |
| d libfuzzer | ts | 2720 | 0 / 15 / 0 | - | 1808 | 0 / 0 / 0 | 9.8e-3 | 1808 | 0 / 0 / 0 | 9.8e-3 |
| d libfuzzer | yaml | 1977 | 0 / 0 / 0 | 4.2e-3 | 1144 | 0 / 0 / 0 | 7.2e-3 | 1144 | 0 / 0 / 0 | 7.2e-3 |
| e fresh options(splice-css) | css | · | · | · | 33593 | 0 / 0 / 0 | 9.1e-5 | 33536 | 0 / 0 / 0 | 9.1e-5 |
| e fresh options(splice-css) | less | · | · | · | 27711 | 0 / 0 / 0 | 1.1e-4 | 27828 | 0 / 0 / 0 | 1.1e-4 |
| e fresh options(splice-css) | scss | · | · | · | 38696 | 0 / 0 / 0 | 7.8e-5 | 38636 | 0 / 0 / 0 | 7.8e-5 |
| e fresh options(splice-js) | js | · | · | · | 61430 | 1 / 0 / 0 | - | 61550 | 0 / 0 / 0 | 5.3e-5 |
| e fresh options(splice-js) | ts | · | · | · | 38570 | 1 / 17 / 0 | - | 38450 | 0 / 2 / 0 | - |
| **total** |  | 1843309 | 2408 / 1257 / 136 | - | 2025638 | 19 / 57 / 0 | - | 2025638 | 0 / 8 / 0 | - |

## Refused share per language

Refused over inputs Prettier formatted, on the streams identical in all three runs (a, a2, b, c).

| lang | run 1 | run 2 | run 3 |
|---|---|---|---|
| css | 0.00% (0/183302) | 0.00% (0/183302) | 0.00% (0/183302) |
| graphql | 0.00% (2/102170) | 0.00% (2/102170) | 0.00% (2/102170) |
| js | 7.91% (35429/448160) | 8.01% (35896/448160) | 8.01% (35900/448160) |
| json | 2.42% (738/30514) | 2.42% (738/30514) | 2.42% (738/30514) |
| less | 0.01% (19/139453) | 0.01% (19/139453) | 0.01% (19/139453) |
| markdown | 0.72% (1328/184106) | 0.72% (1328/184106) | 0.72% (1328/184106) |
| other | 42.41% (81/191) | 42.41% (81/191) | 42.41% (81/191) |
| scss | 0.00% (0/204171) | 0.00% (0/204171) | 0.00% (0/204171) |
| ts | 1.99% (5458/274022) | 2.00% (5491/274022) | 2.02% (5533/274022) |
| yaml | 0.00% (0/97590) | 0.00% (0/97590) | 0.00% (0/97590) |

## Final JS/TS streams (after round 3)

Pooled 3/N over all rows: 3.5e-6 (JS 5.8e-6, TS 8.7e-6).

| stream | lang | N | equal | mismatch | soundness | refused | both_error | panic/hang | 3/N |
|---|---|---|---|---|---|---|---|---|---|
| splice-js, seed 7 | js | 184151 | 168840 | 0 | 0 | 14615 | 696 | 0 | 1.8e-5 |
| splice-js, seed 7 | ts | 115849 | 112774 | 0 | 0 | 2038 | 1037 | 0 | 2.7e-5 |
| options(splice-js), seeds 9/3 | js | 92175 | 84341 | 0 | 0 | 7436 | 398 | 0 | 3.6e-5 |
| options(splice-js), seeds 9/3 | ts | 57825 | 56294 | 0 | 0 | 1021 | 510 | 0 | 5.3e-5 |
| feedback options, JS/TS rows | js | 9560 | 7906 | 0 | 0 | 834 | 820 | 0 | 3.8e-4 |
| feedback options, JS/TS rows | ts | 4675 | 4275 | 0 | 0 | 190 | 210 | 0 | 7.0e-4 |
| feedback options, JS/TS rows, fed back | js | 8091 | 7283 | 0 | 0 | 808 | 0 | 0 | 4.1e-4 |
| feedback options, JS/TS rows, fed back | ts | 4189 | 4016 | 0 | 0 | 172 | 1 | 0 | 7.5e-4 |
| feedback splice-js, seed 8 | js | 61299 | 56270 | 0 | 0 | 4829 | 200 | 0 | 5.3e-5 |
| feedback splice-js, seed 8 | ts | 38701 | 37646 | 0 | 0 | 690 | 365 | 0 | 8.0e-5 |
| feedback splice-js, seed 8, fed back | js | 58625 | 54657 | 0 | 0 | 3897 | 71 | 0 | 5.5e-5 |
| feedback splice-js, seed 8, fed back | ts | 36293 | 35647 | 0 | 0 | 629 | 17 | 0 | 8.4e-5 |
| fresh options(splice-js), seeds 61/62 | js | 61550 | 56286 | 0 | 0 | 5049 | 215 | 0 | 5.3e-5 |
| fresh options(splice-js), seeds 61/62 | ts | 38450 | 37388 | 0 | 0 | 696 | 366 | 0 | 8.0e-5 |
| fresh options(splice-js), seeds 91/92 | js | 92138 | 84381 | 0 | 0 | 7406 | 351 | 0 | 3.6e-5 |
| fresh options(splice-js), seeds 91/92 | ts | 57862 | 56349 | 0 | 0 | 1021 | 492 | 0 | 5.3e-5 |
