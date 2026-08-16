# 70 — What `=`, `<` and `>` mean

**For** CALC-04. **Status:** accepted, implemented.

The comparison operators had two defects that look like one bug and are not.
Both were found by running formulas against Excel's documented rules rather than
by reading the code, and both had survived every test in the crate.

## The two defects

`comparison()` began by trying `as_number()` on both operands and comparing
numerically if *both* succeeded, falling back to `a.cmp(&b)` over the raw text.

**Cross-type coercion.** `Value::as_number` parses text: `Text("1")` becomes
`1.0`, and `Bool` becomes 0 or 1. So `="1"=1` was `TRUE` and `=TRUE=1` was
`TRUE`. Excel says `FALSE` to both. A number and a piece of text that looks like
a number are different values, and a spreadsheet that says otherwise silently
merges two columns of data that a user is keeping apart on purpose.

**Byte-order text comparison.** The fallback compared UTF-8 bytes, so comparison
was case-sensitive and ordered by code point. `="apple"<"Banana"` was `FALSE`
where Excel says `TRUE`, and `=IF(A1="Yes",…)` missed a cell holding `"YES"`.

The sharp edge is that the engine already knew better in one half of itself:
`loose_cmp` and `criterion_matches` in `functions.rs` upper-case before
comparing and refuse to parse text as a number, and `loose_cmp`'s doc comment
claimed it "matches the engine's comparison rules". It did not. So
`COUNTIF(range,"yes")` and `SUM(IF(range="yes",1,0))` disagreed on the same
data — the kind of divergence a user reports as "the numbers don't add up" and
nobody can reproduce, because it depends on which function they reached for.

## The rules, as Excel defines them

Ordering is **by type first, then within type**:

    number  <  text  <  FALSE  <  TRUE

- All numbers sort before all text. All text sorts before all logicals.
- Two numbers compare numerically.
- Two pieces of text compare **case-insensitively**.
- Two logicals compare with `FALSE < TRUE`.
- Values of different types are never equal, and their order is the type order
  above — never a coercion.

**Empty is contextual**, which is the part that cannot be expressed as a rank.
An empty cell compared against a number behaves as `0`, against text as `""`,
and against a logical as `FALSE`. Two empties are equal. This is why empty is
resolved against the *other* operand before ranking, rather than being given a
rank of its own: `=A1=0` and `=A1=""` are both `TRUE` for an empty `A1`, and no
single position in a total order delivers that.

**Errors propagate.** An error on either side is the result, checked before
anything else — comparing against an error is not a question with an answer.

## What this deliberately does not do

**Locale collation.** Case-insensitivity is implemented as ASCII case folding,
which is what `loose_cmp` already does and what the criteria matcher has always
done. Real Excel uses the workbook's locale collation, so `"ä"` sorts by a rule
this does not implement. Naming it here rather than leaving it implied: the
choice is to be consistent with the rest of this engine and with the common
case, not to claim collation support the crate does not have. A locale-aware
comparator is a separate piece of work with its own fidelity dimension, and it
would change sort order for existing documents — so it does not get smuggled in
under a bug fix.

**Text that looks like a number is still text.** `="10"<"9"` is `TRUE`, because
`"1"` precedes `"9"` as text. That is Excel's answer and it surprises people;
it is not a defect.

## Blast radius

Comparison is not local to `IF`. The same operator feeds sorting, autofilter
criteria, conditional-format rules, data validation, `FILTER`, and every
lookup. Changing it changes documents that already exist, which is why the
change is written down rather than made quietly:

- A workbook relying on `="1"=1` being `TRUE` now gets `FALSE`. That workbook
  was getting an answer Excel does not give, so the change moves it toward
  fidelity rather than away.
- A workbook relying on case-sensitive text comparison now matches more rows.
  Excel has no case-sensitive comparison operator — `EXACT()` is the function
  for that, and it is unaffected.

## How it is verified

`comparison_follows_excel_type_ordering` asserts the full matrix in one test:
each of the five cross-type pairs, case-insensitive text equality and ordering,
the empty-cell contextual cases, `FALSE < TRUE`, and error propagation.
`countif_and_a_comparison_agree` asserts the two halves of the engine now give
the same answer over the same data, which is the defect that made this worth
fixing rather than merely wrong.

Both fail if the numeric-coercion-first branch is restored.
