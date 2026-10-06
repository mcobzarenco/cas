# Notes on rules

What was found out about some of the rules, a section to a rule: what the rule does, the
patterns worth keeping, what was measured and how, and what is still open. The rules that have
a name are in [`rules.tsv`](rules.tsv), the library of the app, and the patterns kept with the
app in the folder `patterns` next to it, a file to a rule.

A pattern is written as the app writes it: run-length encoded (`b` dead, `o` alive, `$` next
row) from a corner of the blocks the next step rewrites, at an even generation. Typed or pasted
into the text field of the analysis panel it is studied as it stands here; one cell over it is
another pattern. A period and how far a pattern moves are as `Analyser::fate` gives them, x to
the right and y down; a speed of c/N is a cell in N generations.

* [Weighted Undecided 0](#weighted-undecided-0)
* [15,7,6,3,11,12,4,8,14,13,5,9,10,2,1,0](#1576311124814135910210)
* [Their relatives](#their-relatives)
* [Scribe 0](#scribe-0)

## Weighted Undecided 0

`15,7,6,10,13,12,2,8,14,11,5,4,3,9,1,0`, in the library under this name.

Of the 20 729 rules of the `weighted` family it had the most undecided seeds (23 %) in

```sh
cargo run -p cas-search --release -- --family weighted --generations 500000 --seeds 100 \
    --out weighted-rules.tsv
```

which calls it `confined`. Nothing in it is undecided, and it is not confined: its small
patterns are oscillators and spaceships that take millions of generations to be back. Followed
for 20 000 000 generations (`--rule … --generations 20000000 --blob 0`, a minute and a half)
it is `spaceships`: 84 % of the seeds oscillate, with 38 periods up to 8 765 516, and 14 % are
still not back.

### What the rule does

* **The vacuum flickers** between empty and full, which is why the table changes all sixteen
  blocks. On what differs from the vacuum the rule acts through two tables, one at even and one
  at odd generations, and each changes 9 blocks. With TL, TR, BL, BR the corners of a block:

  | generation | what changes |
  |---|---|
  | even | TL ⇄ BR · TR → TL+BR → BL → TR · TL+TR ⇄ TL+BL · TR+BL ⇄ TL+BL+BR |
  | odd | TR ⇄ TL+BR · TR+BL → TL+TR+BR → TL+BL+BR → TR+BL · TL+TR+BL ⇄ TR+BL+BR · TR+BR ⇄ BL+BR |

* **It keeps a checkerboard weight.** With (0, 0) a corner of the even blocks, a cell where
  x + y is even weighs 1 and one where it is odd weighs 2, in both partitions alike: a cell can
  stay where it is and keep its weight. The search writes this `1·2/2·1`. Every block keeps
  its weight at every step, and the number of cells changes in one way only: a heavy cell
  becomes the two light ones of its block, or the two a heavy one.
* **A lone light cell** (`o`) hops across its block and back: period 4. **A lone heavy cell**
  (`bo`) falls into two light ones, which come together again, part and return: it is back
  after 10 generations, and was never more than 4 cells across.
* **Nothing ever spreads to the right.** At odd generations a block with cells only in its left
  column does not change, so the right edge of a pattern can go a cell forward within its even
  block and no further. Left, up and down there are blocks that carry cells across.
* **It is not linear**: patterns do not superpose, as they do under a rule like rule 90. What a
  cluster of cells does looks like a random permutation of its states: a few long cycles hold
  most of them. On a torus of 4×6 cells (16.7 million configurations, which the rule permutes
  within classes of equal weight) it has 4 194 cycles, the longest of 657 024 turns of two
  generations. Two linear rules on the same torus have longest cycles of 510 and 210, Single
  rotation 15 362, Critters 6 570.
* **Run backwards** it is `15,7,6,10,13,12,4,8,14,11,5,9,3,2,1,0`, the fourth most undecided
  rule of the same table, seen another way round: the same periods, the ships flying along x.
* It has no symmetry: a pattern turned or mirrored is another pattern.

### Patterns

Spaceships, all of weight 8 and all flying along y:

| pattern | period | moves | speed | cells | at most | note |
|---|---|---|---|---|---|---|
| `$o$o$3o` | 427 072 | (0, 4) | c/106 768 | 4 to 8 | 16 across | also `2o2$b3o` |
| `o$o4$bo$2bo3b2o` | 818 160 | (0, 4) | c/204 540 | 4 to 8 | 18 across | what `$bo2$bo$2bo$bo2b2o`, moved a cell to the right, becomes, leaving three light cells behind |
| `bobo$obo` | 7 328 092 | (0, −8) | c/916 012 | 4 to 8 | 18 across | a fifth of all patterns of weight 8; also `2o$3o` |

Their cells lie up to 10 apart, in lots that do not meet for hundreds of thousands of
generations at a time.

Oscillators:

| pattern | period | weight | note |
|---|---|---|---|
| `o` | 4 | 1 | a light cell |
| `bo` | 10 | 2 | a heavy cell |
| `bo$bo` | 2 | 3 | the shortest period there is |
| `b2o` | 52 | 3 | the longest of weight 3 |
| `$o$obo` | 104 | 4 | the longest of weight 4 |
| `$o$o2bo` | 612 | 5 | the longest of weight 5 |
| `4o` | 11 034 | 6 | 18 % of the patterns of weight 6 |
| `$3o$o` | 12 986 | 6 | the longest of weight 6 |
| `4o2$o` | 517 766 | 7 | 14 % of weight 7 |
| `b3o$o` | 1 068 762 | 7 | 41 % of weight 7. `$bo2$bo$2bo$bo2b2o` and `$bo2$2bo$3bobo`, the two patterns that "would not settle", are this oscillator: the second is the first 710 136 generations on, 4 cells lower |
| `$obo$bobo` | 1 996 288 | 8 | |
| `$o$4o` | 2 751 562 | 8 | |
| `3o$obo` | 2 897 646 | 8 | |
| `b3o$o$o` | 8 765 516 | 8 | |
| `bo$2b2o$bo$2bo` | 11 120 730 | 9 | the longest of weight 9 that holds together |
| `b2o3$3o` | 27 101 030 | 8 | |
| `$3o$2o` | 32 782 820 | 8 | 28 % of weight 8; the longest known |

No still life is known.

### What was measured

**Every pattern of up to five cells in a box of 4×4** on a corner of the even blocks, 6 884 of
them, each followed for 40 000 000 generations:

| weight | patterns | oscillators | their periods: how many, the median, the longest | spaceships | a ship and a lone cell | not back |
|---|---|---|---|---|---|---|
| 1 | 8 | 8 | 1 · 4 · 4 | | | |
| 2 | 36 | 36 | 2 · 4 · 10 | | | |
| 3 | 120 | 120 | 7 · 20 · 52 | | | |
| 4 | 322 | 322 | 17 · 32 · 104 | | | |
| 5 | 728 | 728 | 47 · 70 · 612 | | | |
| 6 | 1 400 | 1 400 | 76 · 910 · 12 986 | | | |
| 7 | 2 016 | 2 016 | 71 · 517 766 · 1 068 762 | | | |
| 8 | 1 638 | 1 282 | 83 · 2 897 646 · 32 782 820 | 356 | | |
| 9 | 560 | 79 | 30 · 354 · 11 120 730 | | 143 | 338 |
| 10 | 56 | | | | 12 | 44 |

Twelve of those that were not back, eight of weight 9 and four of weight 10, were followed for
4 000 000 000 generations: each is one of the three ships and a lone cell, which the ship had
not left far enough behind in 40 000 000. Up to weight 8 a cluster holds together, and the
longest period goes up between twofold and eightyfold with every unit of weight; beyond, it
sheds cells.

**A dense blob** (the search's: 64×64 at 30 %, 1 210 cells) spreads, to the left, up and down,
about as the logarithm of time: the box it is in is 86×104 after 1 024 generations, 114×150
after 8 192, 185×230 after 131 072 and 252×296 after 2 097 152, its right edge where it began.
Its cells go from 1 210 to 1 655, heavy ones falling into light ones. The search's 8 000
generations end before any of it has come to the edge of a grid of 256.

**The catcher and the analysis panel** know these patterns: a catch that does
not repeat at once is followed for up to eight million generations, and a small pattern under
study for up to 67 million.

### Open

* Are there ships that fly to the left, or along a diagonal? Heavier ones than weight 8, or
  faster ones? Only boxes of 4×4 and five cells were gone through.
* Why does a cluster of weight 8 hold together and one of weight 9 not?
* What does a blob leave in the end, and how long does that take? Its ships need a hundred
  thousand generations and more to the cell.
* Is 32 782 820 the longest period?

### How

The periods and the ships are confirmed by the analyser of the core (`Analyser::fate` with
`max_generations` set high), which is what the analysis panel and `cas-search` use. The survey
of small patterns, the torus and the blob were made with a scratch program that is not in the
repository: a follower of a few cells as a sorted list, checked against the core's universe
generation by generation, which does ten million generations a second.

## 15,7,6,3,11,12,4,8,14,13,5,9,10,2,1,0

Not in the library. In the table of the `weighted` family it is the most undecided rule that
is called `spaceships` (15 % of its seeds undecided after 500 000 generations, 2 kinds of
spaceship).

### What the rule does

* The same checkerboard weight as [Weighted Undecided 0](#weighted-undecided-0), `1·2/2·1`,
  over a vacuum that flickers. Its two tables change 9 blocks each:

  | generation | what changes |
  |---|---|
  | even | TL ⇄ BR · TR ⇄ TL+BR · TL+TR → BL+BR → TL+BL → TL+TR · TR+BL ⇄ TL+TR+BR |
  | odd | TL+TR → TR+BR → BL+BR → TL+TR · BL ⇄ TL+BR · TR+BL ⇄ TL+BL+BR · TL+TR+BL ⇄ TR+BL+BR |

* A lone light cell (`o`) has period 4, a lone heavy one (`bo`) period 8.
* Cells are carried across blocks all four ways: patterns spread in every direction.
* No symmetry. Run backwards it is `15,7,6,10,11,3,4,8,14,13,5,9,12,2,1,0`, which has the
  same ships flying the other way.

### Patterns

Spaceships, all of weight 7 and all flying along the diagonal:

| pattern | period | moves | speed | note |
|---|---|---|---|---|
| `bo3$ob2o` | 4 490 | (2, 2) | c/2 245 | |
| `$o$o2bo$o` | 7 692 | (2, 2) | c/3 846 | |
| `bo$3o` | 8 894 | (2, 2) | c/4 447 | |
| `bo$obo$o` | 11 062 | (−4, −4) | 2c/5 531 | |
| `4bo$3bo2$5bobo2$bo2$5bobo` | 13 774 | (4, 4) | 2c/6 887 | found by Marius; 5 to 7 cells, 10 across, its cells up to 6 apart. The catcher did not know it until it took loose clouds whole and followed slow catches for longer |
| `$3o2$2bo` | 21 332 | (−2, −2) | c/10 666 | |
| `bo$o$o2bo` | 28 026 | (−8, −8) | 4c/14 013 | |
| `b3o$o` | 417 138 | (10, 10) | 5c/208 569 | the commonest by far: 225 of the 2 516 patterns of up to four cells in a box of 4×4 are it. 20 across, its cells up to 15 apart: at moments further than the catcher takes for one pattern |

A ship of period 1 082 moving (−2, −2) turned up as a piece of something larger, and no
pattern of it was kept.

Oscillators: of the patterns of up to four cells in a box of 4×4, 2 169 are oscillators, the
longest of period 30 640 (`bo3$3o`).

### Open

* Only four cells in a box of 4×4 were gone through, each for 20 000 000 generations.
* What a blob does was not looked at.

## Their relatives

All 87 rules of the `weighted` family that have undecided seeds keep a checkerboard weight
(`1·2/2·1` or `2·1/1·2`), and so do all its 269 `confined` and 128 `frozen` rules: it is the
weighting under which a cell can stay where it is. The other 19 239 rules of the family are
`spaceships` or `gas`.

The fourteen most undecided, each with every pattern of up to four cells in a box of 4×4
followed for 20 000 000 generations (2 516 patterns; a ship counts also where it is a piece of
what came apart). They come in pairs, a rule and the rule that runs it backwards, which the
family holds both of: the same periods, the ships flying the other way, as the canonical form
of each happens to lie.

| rule | undecided at 500 000 | longest period | ships: period and how far they move |
|---|---|---|---|
| `15,7,6,10,13,12,2,8,14,11,5,4,3,9,1,0` | 23 % | 1 996 288 | 7 328 092 (0, −8) |
| `15,7,6,3,13,10,4,8,14,11,5,9,12,2,1,0` | 23 % | 1 506 748 | 1 146 784 (0, 20) · 2 543 658 (0, 32) · 5 707 770 (0, −14) · 8 773 954 (0, −36) |
| `15,7,6,3,13,10,2,8,14,11,5,4,12,9,1,0` | 22 % | 1 506 748 | the same four: the rule before run backwards |
| `15,7,6,10,13,12,4,8,14,11,5,9,3,2,1,0` | 20 % | 1 996 288 | 427 072 (−4, 0) · 7 328 092 (8, 0): the first rule run backwards |
| `15,7,6,5,13,10,4,8,14,11,12,9,3,2,1,0` | 15 % | 3 407 868 | none (backwards it is `15,7,6,12,13,3,2,8,14,11,5,4,10,9,1,0`, further down the table) |
| `15,7,6,3,11,12,4,8,14,13,5,9,10,2,1,0` | 15 % | 30 640 | eight, diagonal: see above |
| `15,7,6,5,11,10,4,8,14,13,3,2,12,9,1,0` | 15 % | 92 956 | 674 (2, −2) · 1 256 (−2, 0) · 12 774 (−2, −2) · 42 120 (0, 2) · 78 184 (2, 2) |
| `15,7,6,3,11,10,2,8,14,13,12,9,5,4,1,0` | 15 % | 92 956 | 674 (−2, 2) · 1 256 (2, 0) · 42 120 (0, −2): the one before run backwards |
| `15,7,6,5,11,10,2,8,14,13,3,4,12,9,1,0` | 13 % | 42 882 | 7 528 (2, 2) · 16 554 (−2, −2) · 46 536 (2, 2) · 143 862 (−2, −2) |
| `15,7,6,10,11,3,4,8,14,13,5,9,12,2,1,0` | 13 % | 30 640 | the sixth rule run backwards |
| `15,7,6,3,11,10,2,8,14,13,12,4,5,9,1,0` | 13 % | 42 882 | the ninth rule run backwards |
| `15,7,6,5,11,10,4,8,14,13,3,9,12,2,1,0` | 12 % | 147 502 | 1 844 620 (0, −4) |
| `15,7,6,3,11,10,4,8,14,13,12,9,5,2,1,0` | 12 % | 147 502 | 1 844 620 (0, 4): the one before run backwards |
| `15,7,6,3,11,12,2,8,14,13,5,4,10,9,1,0` | 11 % | 816 850 | none (backwards it is `15,7,6,10,11,3,2,8,14,13,5,4,12,9,1,0`, further down the table) |

## Scribe 0

`1,0,2,3,10,5,6,7,9,8,4,11,13,12,15,14`, in the library under this name. Its own inverse, with
no symmetry of the square, and canonical as it stands.

Of a million involutions drawn at random and followed for 100 000 000 generations
(`--family involution --generations 100000000 --seeds 1000`, which now needs
`--sample --limit 1000000`), it had the most seeds still undecided by then: 57 %, with 33 %
grown and 10 % oscillating, their periods up to 4. The search calls it `linear`. Nothing in it
grows to speak of: its small patterns build a line of cells, a cell at a time, each taking
about one and a half times as long as the one before.

### What the rule does

* **The vacuum goes through four states**, empty, a cell bottom-right, cells top-left and
  bottom-right, a cell top-left, and is empty again. So the table changes every block, and on
  what differs from the vacuum the rule acts through four tables, one for each generation of
  the cycle. With TL, TR, BL, BR the corners of a block:

  | generation | what changes |
  |---|---|
  | 0 | TR ⇄ TL+TR · TR+BL ⇄ TL+TR+BL · BL → TL+TR+BR → TR+BR → TL+BL → BL |
  | 1 | TR → TL+BL+BR → BL+BR → TL+TR → TR · TR+BR ⇄ TL+TR+BR · TR+BL+BR ⇄ TL+TR+BL+BR |
  | 2 | TR → TL+TR → BL+BR → TL+BL+BR → TR · TR+BR ⇄ TL+TR+BR · TR+BL+BR ⇄ TL+TR+BL+BR |
  | 3 | TR ⇄ TL+TR · TR+BL ⇄ TL+TR+BL · BL → TL+BL → TR+BR → TL+TR+BR → BL |

* **It keeps the cells of one kind and frees the other.** With (0, 0) a corner of the even
  blocks, a cell where x + y is odd is at the top-right or the bottom-left of its block, in
  both partitions, and every one of the four tables keeps the number of such cells in a block:
  a pattern has as many of them for ever, and they are the ones that move. A cell where x + y
  is even, top-left or bottom-right of its block, is left alone by every table when it is alone
  in its block, and is made and unmade next to a moving one. (The search says `not
  conserved`: a weight of 1 and 0 is none it tries.)
* **A lone moving cell** (`bo`) is the smallest pattern that does not repeat. It goes up and to
  the right along the diagonal x + y = const through its block, making and unmaking frozen
  cells beside it, and leaves some behind on that diagonal; then it goes back and forth over
  them, turning them on and off as it passes, and now and then gets one cell further than it
  has been. A head on a tape, which it writes as it reads: what it does next depends on the
  cells it finds, and those are the record of where it has been. After 4 000 000 generations
  the tape is 22 cells long. Nothing goes down or to the left: the box a pattern is in has
  its first block for a corner.
* **How far the head gets grows like the logarithm of time**, and in bursts: the line of
  `bo$2o` first reaches 4 cells out at generation 64, 8 at 576, 12 at 2 368, 16 at 4 928, 20 at
  114 176, 24 at 248 960, 28 at 2 947 584, 32 at 17 306 304 and 36 at 46 967 232: a cell and a
  half for every doubling of the generations, each cell taking one and a half times as long
  as the last on average, though three can come within a few hundred generations and the
  next take ten times as long. A plain binary counter would take exactly twice as long for
  each cell; what this one counts in is not known.

### Patterns

* `o` and `$bo`, a lone frozen cell, and `o$bo`, two on a diagonal: still lifes, as every
  frozen cell is that has no moving cell near it. A pattern leaves many of them behind: at
  4 000 000 generations `bo$2o` is seventeen still lifes and two moving cells, one near where
  it began and one somewhere along the line.
* `bo`, `$o`, `2o`, `o$o`, `3o`, `o$2o`, `2o$bo`: one moving cell, which by 4 000 000 generations
  has a line of 21 or 22 cells.
* `bo$o`, `bo$2o`, `2o$o`, `b2o$2o`, `2o$2o`: two moving cells, and lines of 32 to 34 cells by
  then. Two heads on one tape get further than one.

None of them is worth keeping: the app keeps what comes back to its shape, and these never
have.

### What was measured

**`bo$2o` followed on its own**, at the start of the vacuum's cycle, with the analyser's limits
raised (the fate is undecided at every phase of the cycle alike):

| generations | cells, fewest and most | box | pieces at the end, of which still lifes |
|---|---|---|---|
| 2^18 | 3 to 38 | 28×28 | 14 · 12 |
| 2^20 | 3 to 40 | 28×28 | 12 · 11 |
| 2^22 | 3 to 44 | 32×32 | 16 · 14 |
| 2^24 | 3 to 48 | 34×34 | 17 · 15 |
| 2^26 | 3 to 54 | 40×40 | 19 · 17 |
| 2^28 | 3 to 62 | 44×44 | 19 · 17 |

About 2.9 cells change from one generation to the next, whatever the length of the line.

**The same on a torus of 192×192**, stepped 64 generations at a time, for the first generation
at which the line reached each cell further out (above, under what the rule does) and for
pictures of it: the line lies on the diagonal x + y = 192 through the block the pattern began
in, every cell of it frozen, dense near the beginning and sparse far out, with the two moving
cells on it.

### Open

* Does the line grow for ever? Nothing says it must stop, and nothing says it cannot: a head
  that writes what it reads could in principle run through all the tapes of some length and
  come back to its start, with a period beyond anything that has been followed. The analyser
  calls the pattern undecided at 67 million generations, and would at any number.
* What the head counts in. The times to the next cell are not the doublings of a binary
  counter; a closer look at what the head does over a short tape would say.
* What two heads do to each other: `bo$2o` has one that stays near the beginning. Do they ever
  part, or meet?
* What a blob does. The search's blob ends with four times its cells (`blob 4.10`), which has
  not been looked at.

### How

The fates with `Analyser::study` and `max_generations` set high, a phase at a time; the first
generations and the frontier with `Universe` on a torus, stepping and reading the cells. A
scratch program, not in the repository.
