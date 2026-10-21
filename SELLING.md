# Selling Pro: launch checklist

Pro is **$10 once**, sold through **Polar**. It is built and tested with an offline fake store, and stays switched off until the three Polar values in `src/license.rs` are filled in. Until then every feature is free.

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
- **A landing page:** what the app does, a real screenshot, the price ($10 once), the download link and a contact email.
- **`/eula`**, from `legal/EULA.txt`.
- **`/privacy`**, from `legal/PRIVACY.md`.
- **`/refund`**, from `legal/REFUND.md`.
- **Your legal name as the seller.**

Keep the copy calm and honest. Don't use "your PC is at risk", fake scan numbers or countdowns. Payment providers and Microsoft Defender treat scare-style "optimizers" as unwanted software.

## 3. Polar

**Money.**
- **Fee:** 5% + 50¢ per sale (for organizations created from May 2026), plus 1.5% on cards from outside the US.
- **What you keep:** about **$9.00** of a $10 US sale.
- **Payouts:** Stripe Connect Express to your Indian bank account, in rupees. Expect a small monthly and per-payout fee, plus up to 1% for currency conversion.
- **Merchant of record:** Polar collects and pays US sales tax and EU/UK VAT for you.

**Before you build on it:**
- Polar's account review can take up to 14 days and needs your live website.
- Polar says individuals are fine "if Stripe Connect Express supports the individual type in your region". **Confirm that India individuals are supported first.**
- Email Polar's support with a plain description of the app, a "PC cleanup and memory tool". Ask whether they accept it, and how payouts to India work.

**Setup:**
1. **Create an account.** Sign up at polar.sh and create an organization. Copy its **Organization ID** (Settings).
2. **Create the product.** One product, **one-time price of $10**, called "OneClick Optimizer Pro".
3. **Give it a License Keys benefit.**
   - **Activation limit: 3.** No expiry. A short key prefix such as `OCO` is fine.
   - Copy the benefit's **ID**.
4. **Make a checkout link** for the product and copy its URL.
5. **Try everything in the sandbox first** (sandbox.polar.sh, with test cards). In `src/license.rs`, fill in the sandbox values:
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
6. **Go live.** Swap in the live values with `sandbox: false`. Make one real $10 purchase yourself, then refund it.

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
- [ ] Placeholders in `legal/*` filled in; refund days match `REFUND_DAYS` in `src/ui.rs`
- [ ] Polar approved; in the sandbox: buy, activate, move, refund
- [ ] Live `POLAR` values filled in; version bumped (v0.4.0); CHANGELOG updated
- [ ] Ideally, the exe is signed
- [ ] CA consulted on GST, LUT, income tax and bank reporting
