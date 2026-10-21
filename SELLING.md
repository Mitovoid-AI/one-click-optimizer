# Selling Pro: launch checklist

Pro is **$20 lifetime** (pay once), with a **$10 lifetime launch price until 31 January 2027**, sold through **Polar**. It is built and tested with an offline fake store, and stays switched off until the three Polar values in `src/license.rs` are filled in. Until then every feature is free.

> This is not legal or tax advice. Check the tax and legal steps with a Chartered Accountant (CA) before the first sale.

## What Pro unlocks

| | Free | Pro |
|---|---|---|
| Scan, health score, every detail page, fixes one by one, undo | Yes | Yes |
| One-click Optimise (window and tray) | 3 times | Unlimited |
| Developer junk | See it | Clean it |
| Autopilot (scheduled clean-ups) | | Yes |
| Focus mode | | Yes |

One key works on up to 3 PCs. The app enforces this through Polar's activation limit.

## 1. Name

"OneClick Optimizer" is already used by Bitdefender (a feature of Total Security). Ashampoo WinOptimizer has a "One-Click Optimizer" module, and the Microsoft Store has "1Click Optimizer". Renaming before launch is cheapest.

1. Pick a distinct name.
2. Search it on USPTO, EUIPO TMview, UK IPO, the WIPO Global Brand Database and IP India, in Classes 9 and 42.
3. Check that the domain and Store name are free.

The name lives in `APP_NAME` (`src/lib.rs`), the window and tray class names, `assets/` and the docs.

## 2. Website (GitHub Pages is fine)

Polar reviews your site before it pays out. It needs:
- **A landing page:** what the app does, a real screenshot, the price, clearly marked lifetime ($20 lifetime, $10 until 31 January 2027), the download link and a contact email.
- **`/terms`** (and `/eula`), from `legal/EULA.txt`.
- **`/privacy`**, from `legal/PRIVACY.md`.
- **`/refund`**, from `legal/REFUND.md`.
- **Your legal name as the seller.**

`python tools/legal-pages.py` builds all of these, plus `/licenses` and the footer, from `legal/`. Fill in your name, city and email at the top of the script first.

Keep the copy calm and honest. Don't use "your PC is at risk", fake scan numbers or countdowns. Payment providers and Microsoft Defender treat scare-style "optimizers" as unwanted software.

## 3. Polar

**Money.**
- **Fee:** 5% + 50¢ per sale (for organizations created from May 2026), plus 1.5% on cards from outside the US.
- **What you keep:** about **$9.00** of a $10 US sale (the launch price), and about **$18.50** of a $20 sale.
- **Payouts:** Stripe Connect Express to your Indian bank account, in rupees. Expect a small monthly and per-payout fee, plus up to 1% for currency conversion.
- **Launch price:** $10 until 31 January 2027, then $20, and the $20 must really start on 1 February. A crossed-out price that was never charged is a fake reference price, which is illegal in the US, the EU, the UK and India. A launch price that really ends is fine. The app and the website switch to $20 by themselves at 1 February 2027, 00:00 UTC (`LAUNCH_ENDS` in `src/license.rs`, `Date.UTC(2027, 1, 1)` in `docs/index.html`).
- **Merchant of record:** Polar collects and pays US sales tax and EU/UK VAT for you.
- **Refunds:** sales are final (`legal/REFUND.md`). The exceptions are a key that won't activate or Pro not working on the buyer's PC (fix it or refund, within 30 days) and refunds the law requires. Polar can still refund an order itself within 60 days to head off a chargeback, even under a no-refund policy. A bank dispute costs $15 even if you win, and Polar holds accounts to a 0.4% chargeback rate. Answer support threads Polar loops you into within 48 hours, or Polar may refund the customer itself. So refunding a genuinely unhappy buyer is the cheaper option.

**Before you build on it:**
- Polar's account review can take up to 14 days and needs your live website.
- Polar says individuals are fine "if Stripe Connect Express supports the individual type in your region". **Confirm that India individuals are supported first.**
- Email Polar's support with a plain description of the app, a "PC cleanup and memory tool". Ask whether they accept it, and how payouts to India work.

**Setup:**
1. **Create an account.** Sign up at polar.sh and create an organization. Copy its **Organization ID** (Settings).
2. **Create the product.** One product, **one-time price of $20**, called "OneClick Optimizer Pro".
3. **Give it a License Keys benefit.**
   - **Activation limit: 3.** No expiry. A short key prefix such as `OCO` is fine.
   - Copy the benefit's **ID**.
4. **Add the launch discount** (Products → Discounts):
   - Name: `Launch price`. Buyers see this name at checkout.
   - **50%** off. Leave **Code empty**, so it only comes through the checkout link.
   - Restricted to this product.
   - **Ends at 1 February 2027, 00:00 UTC** (05:30 in India), the same moment the app and the website switch to $20.
5. **Make a checkout link** for the product, set **Discount** to "Launch price", and copy its URL.
6. **Try everything in the sandbox first** (sandbox.polar.sh, with test cards). In `src/license.rs`, fill in the sandbox values:
   ```rust
   const POLAR: Store = Store {
       provider: Provider::Polar { sandbox: true },
       checkout_url: "https://sandbox.polar.sh/checkout/...",
       product_id: "<license key benefit ID>",
       org_id: "<organization ID>",
       price: PRICE,
   };
   ```
   Build it, then buy, activate, use "Move Pro to another PC", and refund. After the refund, activating the key again must fail. Installed copies drop a refunded key at their next re-check, which runs every 14 days.
7. **Go live.** Swap in the live values with `sandbox: false`. **Never test with a real card.** Polar treats that as card testing and can block the account. Instead:
   - Create a second discount, "Owner test": **100%**, max redemptions **1**, no code.
   - Create a second checkout link with it preset. Buy Pro for $0 with your own email.
   - With that key: activate, use "Move Pro to another PC", activate again, then revoke the key in Polar. After the 14-day re-check, or on a fresh activation, it must stop working.

The app sends Polar only the key, your organization ID and an anonymous PC tag. It never sends the PC name.

**If Polar says no:** the app also supports Dodo Payments (`Provider::Dodo`) and Lemon Squeezy (`Provider::LemonSqueezy`), so switching takes a few lines. Don't use Paddle (it bans "device cleaners") or Creem (its license API needs a secret key).

## 4. Code signing (cuts the SmartScreen warning)

An unsigned optimizer that closes programs and edits startup entries looks suspicious. Options:
- **Azure Artifact Signing**, about $9.99 a month. At launch it was open to individuals only in the US and Canada, so check whether India is supported yet.
- **A standard code-signing certificate** sold to individuals, for example by Certum. Check its price and its identity checks for India.

## 5. India: tax and money (confirm with a CA)

- **GST**
  - Registration is mandatory above ₹20 lakh a year. Ask whether you need it below that, since exports count as inter-state supplies.
  - If you register, file a **LUT** (form RFD-11) every year so export sales are zero-rated.
- **Income tax**
  - Profit is business income.
  - Ask about presumptive taxation: s.44AD at 6% of digital receipts, or 44ADA. Since 1 Apr 2026 the Income-tax Act 2025 merges them into Section 58.
  - Ask which ITR form to use (ITR-3 or ITR-4) and the advance-tax dates.
- **FEMA and RBI.** New export rules apply from 1 Oct 2026.
  - Proceeds must be realised within the deadline (reported as 9 months).
  - Your bank reports a monthly EDF.
  - Ask your bank which purpose code to use (often P0802 for software) and how to document payouts that come through Stripe for Polar.
  - Keep every FIRA or bank advice.
- **US forms.** Fill in the W-8BEN when asked, with your PAN as the foreign tax ID. Because Polar is the merchant of record, you have no US sales-tax filing.
- **Records.** Keep payout statements, invoices and FIRAs for at least 8 years.

## 6. Before the first sale

- [ ] Name cleared; site live with EULA, Privacy and Refund pages
- [ ] Placeholders in `legal/*` filled in; refund wording matches `REFUND_NOTE` in `src/ui.rs` and EULA section 4
- [ ] Polar approved; in the sandbox: buy, activate, move, refund
- [ ] Live `POLAR` values filled in; version bumped (v0.4.0); CHANGELOG updated
- [ ] Ideally, the exe is signed
- [ ] CA consulted on GST, LUT, income tax and bank reporting
