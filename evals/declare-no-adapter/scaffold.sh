#!/bin/sh
# Eval scaffold: a tiny Go project, a language proob has no adapter for, as a
# fresh git repository in the case workspace, with a development build of the
# proob binary at ./.proob-bin/proob, found as the Rust scaffolds find it:
# evals/.bin/proob (gitignored copy of a build), else target/debug/proob, else
# $PROOB_BIN, else proob on PATH.
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
plugin="${CLAUDE_PLUGIN_ROOT:-$(CDPATH= cd -- "$here/../.." && pwd)}"
bin=""
for cand in "$plugin/evals/.bin/proob" "$plugin/target/debug/proob" "${PROOB_BIN:-}" "$(command -v proob 2>/dev/null || true)"; do
  [ -n "$cand" ] && [ -x "$cand" ] && { bin=$cand; break; }
done
[ -n "$bin" ] || { echo "scaffold: no proob binary; build it (cargo build --locked -p proob) and copy it to evals/.bin/proob" >&2; exit 1; }
mkdir -p .proob-bin docs/incidents && cp "$bin" .proob-bin/proob
printf '/.proob-bin/\n' > .gitignore
cat > go.mod <<'GO'
module example.com/till

go 1.22
GO
cat > README.md <<'MD'
# till

A cash till for a small shop: open a shift with a float, take deposits and
withdrawals in cents, close the shift with a report. The shift report must
balance: the closing amount equals the float plus every deposit minus every
withdrawal, and the till never goes negative.
MD
cat > till.go <<'GO'
// Package till models one cash drawer across a shift.
package till

import "errors"

// ErrInsufficient is returned when a withdrawal would take the till below zero.
var ErrInsufficient = errors.New("till: insufficient cash")

// Till is a cash drawer. Amounts are cents; the balance must never be negative
// (see docs/incidents/2026-02-negative-till.md).
type Till struct {
	float   int64
	balance int64
	in, out int64
	open    bool
}

// Open starts a shift with the given float; a negative float is clamped to zero.
func Open(float int64) *Till {
	if float < 0 {
		float = 0
	}
	return &Till{float: float, balance: float, open: true}
}

// Deposit adds cents to the till. Zero and negative deposits are ignored.
func (t *Till) Deposit(cents int64) {
	if !t.open || cents <= 0 {
		return
	}
	t.balance += cents
	t.in += cents
}

// Withdraw removes cents from the till, never below zero.
func (t *Till) Withdraw(cents int64) error {
	if !t.open || cents <= 0 {
		return nil
	}
	if cents > t.balance {
		return ErrInsufficient
	}
	t.balance -= cents
	t.out += cents
	return nil
}

// Report is what the shift closes with; Closing must equal Float + In - Out.
type Report struct {
	Float, In, Out, Closing int64
}

// Close ends the shift. A closed till accepts no further movements.
func (t *Till) Close() Report {
	t.open = false
	return Report{Float: t.float, In: t.in, Out: t.out, Closing: t.balance}
}
GO
cat > till_test.go <<'GO'
package till

import "testing"

func TestReportBalances(t *testing.T) {
	till := Open(1000)
	till.Deposit(250)
	if err := till.Withdraw(400); err != nil {
		t.Fatal(err)
	}
	r := till.Close()
	if r.Closing != r.Float+r.In-r.Out {
		t.Fatalf("report does not balance: %+v", r)
	}
}

func TestNeverNegative(t *testing.T) {
	till := Open(100)
	if err := till.Withdraw(101); err != ErrInsufficient {
		t.Fatalf("expected ErrInsufficient, got %v", err)
	}
	if till.Close().Closing != 100 {
		t.Fatal("balance changed on a refused withdrawal")
	}
}
GO
cat > docs/incidents/2026-02-negative-till.md <<'MD'
# 2026-02: a till closed negative

A refund larger than the drawer was accepted, the shift report showed a
negative closing amount, and the day's reconciliation failed. Fix: a
withdrawal that would take the balance below zero is refused and the balance
is left unchanged.
MD
[ -d .git ] || git init -q -b main
git -c user.name=eval -c user.email=eval@example.com add -A
git -c user.name=eval -c user.email=eval@example.com commit -q -m "fixture"
