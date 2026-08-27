//! The built-in function library (starter subset). Aggregates flatten ranges to
//! numbers; `IF` evaluates only the taken branch.

use std::cmp::Ordering;

use casual_calc_formula::Expr;
use casual_calc_model::{CellRef, ErrorValue};

use crate::eval::Evaluator;
use crate::value::{Value, number_to_text};

// The families this library splits into. Each is a section heading the old
// single file already carried; `MNT-002` made the module system enforce what
// the comments had been asserting. Re-exported flat so the dispatch below —
// and every sibling — names a function exactly as it always did.
mod aggregate;
mod datetime;
mod engineering;
mod financial;
mod info;
mod lookup;
mod math;
mod matrix;
mod special;
mod stats;
mod text;

pub(crate) use aggregate::*;
pub(crate) use datetime::*;
pub(crate) use engineering::*;
pub(crate) use financial::*;
pub(crate) use info::*;
pub(crate) use lookup::*;
pub(crate) use math::*;
pub(crate) use matrix::*;
pub(crate) use special::*;
pub(crate) use stats::*;
pub(crate) use text::*;

/// Guard against pathological full-range aggregates (a dependency-graph with
/// range buckets is the Phase-2 optimization; this bounds the naive scan).
pub(crate) const MAX_RANGE_CELLS: u64 = 2_000_000;

/// Whether a name is a builtin, so a defined name cannot shadow one.
///
/// A LAMBDA named `SUM` must not replace `SUM` — Excel refuses the name, and
/// silently preferring the user's would change every existing formula in the
/// file.
#[must_use]
pub fn is_builtin(name: &str) -> bool {
    FUNCTIONS.binary_search_by(|(n, _)| (*n).cmp(name)).is_ok()
}

/// The catalog of built-in functions as `(name, signature)`, kept alphabetical.
/// This is the **single source of truth** for the function list — the host UI
/// (autocomplete / signature help) reads it via the SDK/WASM instead of keeping
/// its own copy, and a test asserts every entry has a dispatch arm in
/// `call_function` so the two never drift. Add a function in both places.
pub const FUNCTIONS: &[(&str, &str)] = &[
    ("ABS", "ABS(number)"),
    (
        "ACCRINT",
        "ACCRINT(issue, first_interest, settlement, rate, par, frequency, [basis], [calc_method])",
    ),
    (
        "ACCRINTM",
        "ACCRINTM(issue, settlement, rate, par, [basis])",
    ),
    ("ACOS", "ACOS(number)"),
    ("ACOSH", "ACOSH(number)"),
    ("ADDRESS", "ADDRESS(row, column, [abs], [a1], [sheet])"),
    (
        "AMORDEGRC",
        "AMORDEGRC(cost, date_purchased, first_period, salvage, period, rate, [basis])",
    ),
    (
        "AMORLINC",
        "AMORLINC(cost, date_purchased, first_period, salvage, period, rate, [basis])",
    ),
    ("AND", "AND(logical1, …)"),
    ("AREAS", "AREAS(reference)"),
    ("ASC", "ASC(text)"),
    ("ASIN", "ASIN(number)"),
    ("ASINH", "ASINH(number)"),
    ("ATAN", "ATAN(number)"),
    ("ATAN2", "ATAN2(x, y)"),
    ("ATANH", "ATANH(number)"),
    ("AVEDEV", "AVEDEV(number1, …)"),
    ("AVERAGE", "AVERAGE(number1, …)"),
    ("AVERAGEA", "AVERAGEA(value1, …)"),
    ("AVERAGEIF", "AVERAGEIF(range, criteria, [average_range])"),
    ("AVERAGEIFS", "AVERAGEIFS(avg_range, range1, criteria1, …)"),
    ("BAHTTEXT", "BAHTTEXT(number)"),
    ("BESSELI", "BESSELI(x, n)"),
    ("BESSELJ", "BESSELJ(x, n)"),
    ("BESSELK", "BESSELK(x, n)"),
    ("BESSELY", "BESSELY(x, n)"),
    ("BETA.INV", "BETA.INV(probability, alpha, beta, [A], [B])"),
    ("BETADIST", "BETADIST(x, alpha, beta, [A], [B])"),
    ("BETAINV", "BETAINV(probability, alpha, beta, [A], [B])"),
    ("BIN2DEC", "BIN2DEC(number)"),
    ("BIN2HEX", "BIN2HEX(number, [places])"),
    ("BIN2OCT", "BIN2OCT(number, [places])"),
    (
        "BINOM.DIST",
        "BINOM.DIST(number_s, trials, probability_s, cumulative)",
    ),
    (
        "BINOMDIST",
        "BINOMDIST(number_s, trials, probability_s, cumulative)",
    ),
    ("BITAND", "BITAND(number1, number2)"),
    ("BITLSHIFT", "BITLSHIFT(number, shift)"),
    ("BITOR", "BITOR(number1, number2)"),
    ("BITRSHIFT", "BITRSHIFT(number, shift)"),
    ("BITXOR", "BITXOR(number1, number2)"),
    ("BYCOL", "BYCOL(array, lambda)"),
    ("BYROW", "BYROW(array, lambda)"),
    ("CEILING", "CEILING(number, significance)"),
    ("CELL", "CELL(info_type, [reference])"),
    ("CHAR", "CHAR(number)"),
    ("CHIDIST", "CHIDIST(x, degrees_freedom)"),
    ("CHIINV", "CHIINV(probability, degrees_freedom)"),
    ("CHISQ.DIST.RT", "CHISQ.DIST.RT(x, degrees_freedom)"),
    ("CHISQ.INV.RT", "CHISQ.INV.RT(probability, degrees_freedom)"),
    ("CHISQ.TEST", "CHISQ.TEST(actual, expected)"),
    ("CHITEST", "CHITEST(actual, expected)"),
    ("CHOOSE", "CHOOSE(index, value1, …)"),
    ("CLEAN", "CLEAN(text)"),
    ("CODE", "CODE(text)"),
    ("COLUMN", "COLUMN([reference])"),
    ("COLUMNS", "COLUMNS(array)"),
    ("COMBIN", "COMBIN(n, k)"),
    ("COMBINA", "COMBINA(n, k)"),
    ("COMPLEX", "COMPLEX(real, i, [suffix])"),
    ("CONCAT", "CONCAT(text1, …)"),
    ("CONCATENATE", "CONCATENATE(text1, …)"),
    ("CONFIDENCE", "CONFIDENCE(alpha, standard_dev, size)"),
    (
        "CONFIDENCE.NORM",
        "CONFIDENCE.NORM(alpha, standard_dev, size)",
    ),
    ("CONVERT", "CONVERT(number, from_unit, to_unit)"),
    ("CORREL", "CORREL(array1, array2)"),
    ("COS", "COS(number)"),
    ("COSH", "COSH(number)"),
    ("COT", "COT(number)"),
    ("COTH", "COTH(number)"),
    ("COUNT", "COUNT(value1, …)"),
    ("COUNTA", "COUNTA(value1, …)"),
    ("COUNTBLANK", "COUNTBLANK(range)"),
    ("COUNTIF", "COUNTIF(range, criteria)"),
    ("COUNTIFS", "COUNTIFS(range1, criteria1, …)"),
    (
        "COUPDAYBS",
        "COUPDAYBS(settlement, maturity, frequency, [basis])",
    ),
    (
        "COUPDAYS",
        "COUPDAYS(settlement, maturity, frequency, [basis])",
    ),
    (
        "COUPDAYSNC",
        "COUPDAYSNC(settlement, maturity, frequency, [basis])",
    ),
    (
        "COUPNCD",
        "COUPNCD(settlement, maturity, frequency, [basis])",
    ),
    (
        "COUPNUM",
        "COUPNUM(settlement, maturity, frequency, [basis])",
    ),
    (
        "COUPPCD",
        "COUPPCD(settlement, maturity, frequency, [basis])",
    ),
    ("COVAR", "COVAR(array1, array2)"),
    ("COVARIANCE.P", "COVARIANCE.P(array1, array2)"),
    ("CRITBINOM", "CRITBINOM(trials, probability_s, alpha)"),
    ("CSC", "CSC(number)"),
    ("CSCH", "CSCH(number)"),
    ("CUMIPMT", "CUMIPMT(rate, nper, pv, start, end, type)"),
    ("CUMPRINC", "CUMPRINC(rate, nper, pv, start, end, type)"),
    ("DATE", "DATE(year, month, day)"),
    ("DATEDIF", "DATEDIF(start, end, unit)"),
    ("DATEVALUE", "DATEVALUE(date_text)"),
    ("DAVERAGE", "DAVERAGE(database, field, criteria)"),
    ("DAY", "DAY(serial_number)"),
    ("DAYS", "DAYS(end_date, start_date)"),
    ("DAYS360", "DAYS360(start, end, [method])"),
    ("DB", "DB(cost, salvage, life, period, [month])"),
    ("DCOUNT", "DCOUNT(database, field, criteria)"),
    ("DCOUNTA", "DCOUNTA(database, field, criteria)"),
    ("DDB", "DDB(cost, salvage, life, period, [factor])"),
    ("DEC2BIN", "DEC2BIN(number, [places])"),
    ("DEC2HEX", "DEC2HEX(number, [places])"),
    ("DEC2OCT", "DEC2OCT(number, [places])"),
    ("DEGREES", "DEGREES(angle)"),
    ("DELTA", "DELTA(number1, [number2])"),
    ("DEVSQ", "DEVSQ(number1, …)"),
    ("DGET", "DGET(database, field, criteria)"),
    (
        "DISC",
        "DISC(settlement, maturity, pr, redemption, [basis])",
    ),
    ("DMAX", "DMAX(database, field, criteria)"),
    ("DMIN", "DMIN(database, field, criteria)"),
    ("DOLLAR", "DOLLAR(number, [decimals])"),
    ("DOLLARDE", "DOLLARDE(fractional_dollar, fraction)"),
    ("DOLLARFR", "DOLLARFR(decimal_dollar, fraction)"),
    ("DPRODUCT", "DPRODUCT(database, field, criteria)"),
    ("DSTDEV", "DSTDEV(database, field, criteria)"),
    ("DSTDEVP", "DSTDEVP(database, field, criteria)"),
    ("DSUM", "DSUM(database, field, criteria)"),
    (
        "DURATION",
        "DURATION(settlement, maturity, coupon, yld, frequency, [basis])",
    ),
    ("DVAR", "DVAR(database, field, criteria)"),
    ("DVARP", "DVARP(database, field, criteria)"),
    ("ECMA.CEILING", "ECMA.CEILING(number, significance)"),
    ("EDATE", "EDATE(start_date, months)"),
    ("EFFECT", "EFFECT(nominal_rate, npery)"),
    ("EOMONTH", "EOMONTH(start_date, months)"),
    ("ERF", "ERF(lower, [upper])"),
    ("ERFC", "ERFC(x)"),
    ("ERROR.TYPE", "ERROR.TYPE(error)"),
    ("EVEN", "EVEN(number)"),
    ("EXACT", "EXACT(text1, text2)"),
    ("EXP", "EXP(number)"),
    ("EXPON.DIST", "EXPON.DIST(x, lambda, cumulative)"),
    ("EXPONDIST", "EXPONDIST(x, lambda, cumulative)"),
    (
        "F.DIST.RT",
        "F.DIST.RT(x, degrees_freedom1, degrees_freedom2)",
    ),
    (
        "F.INV.RT",
        "F.INV.RT(probability, deg_freedom1, deg_freedom2)",
    ),
    ("F.TEST", "F.TEST(array1, array2)"),
    ("FACT", "FACT(number)"),
    ("FACTDOUBLE", "FACTDOUBLE(number)"),
    ("FALSE", "FALSE()"),
    ("FDIST", "FDIST(x, degrees_freedom1, degrees_freedom2)"),
    ("FILTER", "FILTER(array, include, [if_empty])"),
    ("FIND", "FIND(find_text, within_text, [start])"),
    ("FINDB", "FINDB(find_text, within_text, [start_num])"),
    ("FINV", "FINV(probability, deg_freedom1, deg_freedom2)"),
    ("FISHER", "FISHER(x)"),
    ("FISHERINV", "FISHERINV(y)"),
    ("FIXED", "FIXED(number, [decimals], [no_commas])"),
    ("FLOOR", "FLOOR(number, significance)"),
    ("FORECAST", "FORECAST(x, known_y, known_x)"),
    ("FREQUENCY", "FREQUENCY(data_array, bins_array)"),
    ("FTEST", "FTEST(array1, array2)"),
    ("FV", "FV(rate, nper, pmt, [pv], [type])"),
    ("FVSCHEDULE", "FVSCHEDULE(principal, schedule)"),
    ("GAMMA.DIST", "GAMMA.DIST(x, alpha, beta, cumulative)"),
    ("GAMMA.INV", "GAMMA.INV(probability, alpha, beta)"),
    ("GAMMADIST", "GAMMADIST(x, alpha, beta, cumulative)"),
    ("GAMMAINV", "GAMMAINV(probability, alpha, beta)"),
    ("GAMMALN", "GAMMALN(x)"),
    ("GCD", "GCD(number1, …)"),
    ("GEOMEAN", "GEOMEAN(number1, …)"),
    ("GESTEP", "GESTEP(number, [step])"),
    (
        "GETPIVOTDATA",
        "GETPIVOTDATA(data_field, pivot_table, [field, item], …)",
    ),
    ("GROWTH", "GROWTH(known_y, [known_x], [new_x], [const])"),
    ("HARMEAN", "HARMEAN(number1, …)"),
    ("HEX2BIN", "HEX2BIN(number, [places])"),
    ("HEX2DEC", "HEX2DEC(number)"),
    ("HEX2OCT", "HEX2OCT(number, [places])"),
    ("HLOOKUP", "HLOOKUP(lookup, table, row, [exact])"),
    ("HOUR", "HOUR(serial_number)"),
    ("HYPERLINK", "HYPERLINK(link, [friendly])"),
    (
        "HYPGEOMDIST",
        "HYPGEOMDIST(sample_s, number_sample, population_s, number_pop)",
    ),
    ("IF", "IF(logical_test, value_if_true, value_if_false)"),
    ("IFERROR", "IFERROR(value, value_if_error)"),
    ("IFNA", "IFNA(value, value_if_na)"),
    ("IFS", "IFS(test1, value1, …)"),
    ("IMABS", "IMABS(inumber)"),
    ("IMAGINARY", "IMAGINARY(inumber)"),
    ("IMARGUMENT", "IMARGUMENT(inumber)"),
    ("IMCONJUGATE", "IMCONJUGATE(inumber)"),
    ("IMCOS", "IMCOS(inumber)"),
    ("IMCOSH", "IMCOSH(inumber)"),
    ("IMDIV", "IMDIV(inumber1, inumber2)"),
    ("IMEXP", "IMEXP(inumber)"),
    ("IMLN", "IMLN(inumber)"),
    ("IMLOG10", "IMLOG10(inumber)"),
    ("IMLOG2", "IMLOG2(inumber)"),
    ("IMPOWER", "IMPOWER(inumber, number)"),
    ("IMPRODUCT", "IMPRODUCT(inumber1, …)"),
    ("IMREAL", "IMREAL(inumber)"),
    ("IMSIN", "IMSIN(inumber)"),
    ("IMSINH", "IMSINH(inumber)"),
    ("IMSQRT", "IMSQRT(inumber)"),
    ("IMSUB", "IMSUB(inumber1, inumber2)"),
    ("IMSUM", "IMSUM(inumber1, …)"),
    ("IMTAN", "IMTAN(inumber)"),
    ("INDEX", "INDEX(array, row_num, [col_num])"),
    ("INDIRECT", "INDIRECT(ref_text, [a1])"),
    ("INFO", "INFO(type_text)"),
    ("INT", "INT(number)"),
    ("INTERCEPT", "INTERCEPT(known_y, known_x)"),
    (
        "INTRATE",
        "INTRATE(settlement, maturity, investment, redemption, [basis])",
    ),
    ("IPMT", "IPMT(rate, per, nper, pv, [fv], [type])"),
    ("IRR", "IRR(values, [guess])"),
    ("ISBLANK", "ISBLANK(value)"),
    ("ISERR", "ISERR(value)"),
    ("ISERROR", "ISERROR(value)"),
    ("ISEVEN", "ISEVEN(number)"),
    ("ISFORMULA", "ISFORMULA(reference)"),
    ("ISLOGICAL", "ISLOGICAL(value)"),
    ("ISNA", "ISNA(value)"),
    ("ISNONTEXT", "ISNONTEXT(value)"),
    ("ISNUMBER", "ISNUMBER(value)"),
    ("ISO.CEILING", "ISO.CEILING(number, [significance])"),
    ("ISODD", "ISODD(number)"),
    ("ISOMITTED", "ISOMITTED(argument)"),
    ("ISOWEEKNUM", "ISOWEEKNUM(date)"),
    ("ISPMT", "ISPMT(rate, per, nper, pv)"),
    ("ISREF", "ISREF(value)"),
    ("ISTEXT", "ISTEXT(value)"),
    ("JIS", "JIS(text)"),
    ("KURT", "KURT(number1, …)"),
    ("LAMBDA", "LAMBDA(parameter, …, calculation)"),
    ("LARGE", "LARGE(array, k)"),
    ("LCM", "LCM(number1, …)"),
    ("LEFT", "LEFT(text, [num_chars])"),
    ("LEFTB", "LEFTB(text, [num_bytes])"),
    ("LEN", "LEN(text)"),
    ("LENB", "LENB(text)"),
    ("LET", "LET(name1, value1, …, calculation)"),
    ("LINEST", "LINEST(known_y, [known_x], [const], [stats])"),
    ("LN", "LN(number)"),
    ("LOG", "LOG(number, [base])"),
    ("LOG10", "LOG10(number)"),
    ("LOGEST", "LOGEST(known_y, [known_x], [const], [stats])"),
    ("LOGINV", "LOGINV(probability, mean, standard_dev)"),
    (
        "LOGNORM.DIST",
        "LOGNORM.DIST(x, mean, standard_dev, cumulative)",
    ),
    (
        "LOGNORM.INV",
        "LOGNORM.INV(probability, mean, standard_dev)",
    ),
    ("LOGNORMDIST", "LOGNORMDIST(x, mean, standard_dev)"),
    ("LOOKUP", "LOOKUP(value, vector, [result])"),
    ("LOWER", "LOWER(text)"),
    ("MAKEARRAY", "MAKEARRAY(rows, columns, lambda)"),
    ("MAP", "MAP(array, lambda)"),
    ("MATCH", "MATCH(lookup, array, [match_type])"),
    ("MAX", "MAX(number1, …)"),
    ("MAXA", "MAXA(value1, …)"),
    ("MAXIFS", "MAXIFS(max_range, range1, criteria1, …)"),
    ("MDETERM", "MDETERM(array)"),
    (
        "MDURATION",
        "MDURATION(settlement, maturity, coupon, yld, frequency, [basis])",
    ),
    ("MEDIAN", "MEDIAN(number1, …)"),
    ("MID", "MID(text, start_num, num_chars)"),
    ("MIDB", "MIDB(text, start_num, num_bytes)"),
    ("MIN", "MIN(number1, …)"),
    ("MINA", "MINA(value1, …)"),
    ("MINIFS", "MINIFS(min_range, range1, criteria1, …)"),
    ("MINUTE", "MINUTE(serial_number)"),
    ("MINVERSE", "MINVERSE(array)"),
    ("MIRR", "MIRR(values, finance_rate, reinvest_rate)"),
    ("MMULT", "MMULT(array1, array2)"),
    ("MOD", "MOD(number, divisor)"),
    ("MODE", "MODE(number1, …)"),
    ("MODE.SNGL", "MODE.SNGL(number1, …)"),
    ("MONTH", "MONTH(serial_number)"),
    ("MROUND", "MROUND(number, multiple)"),
    ("MULTINOMIAL", "MULTINOMIAL(number1, …)"),
    ("N", "N(value)"),
    ("NA", "NA()"),
    (
        "NEGBINOMDIST",
        "NEGBINOMDIST(number_f, number_s, probability_s)",
    ),
    ("NETWORKDAYS", "NETWORKDAYS(start, end, [holidays])"),
    (
        "NETWORKDAYS.INTL",
        "NETWORKDAYS.INTL(start, end, [weekend], [holidays])",
    ),
    ("NOMINAL", "NOMINAL(effect_rate, npery)"),
    ("NORM.DIST", "NORM.DIST(x, mean, sd, cumulative)"),
    ("NORM.INV", "NORM.INV(probability, mean, sd)"),
    ("NORM.S.DIST", "NORM.S.DIST(z, cumulative)"),
    ("NORM.S.INV", "NORM.S.INV(probability)"),
    ("NORMDIST", "NORMDIST(x, mean, sd, cumulative)"),
    ("NORMINV", "NORMINV(probability, mean, sd)"),
    ("NORMSDIST", "NORMSDIST(z)"),
    ("NORMSINV", "NORMSINV(probability)"),
    ("NOT", "NOT(logical)"),
    ("NOW", "NOW()"),
    ("NPER", "NPER(rate, pmt, pv, [fv], [type])"),
    ("NPV", "NPV(rate, value1, …)"),
    (
        "NUMBERVALUE",
        "NUMBERVALUE(text, [decimal_separator], [group_separator])",
    ),
    ("OCT2BIN", "OCT2BIN(number, [places])"),
    ("OCT2DEC", "OCT2DEC(number)"),
    ("OCT2HEX", "OCT2HEX(number, [places])"),
    ("ODD", "ODD(number)"),
    (
        "ODDFPRICE",
        "ODDFPRICE(settlement, maturity, issue, first_coupon, rate, yld, redemption, frequency, [basis])",
    ),
    (
        "ODDFYIELD",
        "ODDFYIELD(settlement, maturity, issue, first_coupon, rate, pr, redemption, frequency, [basis])",
    ),
    (
        "ODDLPRICE",
        "ODDLPRICE(settlement, maturity, last_interest, rate, yld, redemption, frequency, [basis])",
    ),
    (
        "ODDLYIELD",
        "ODDLYIELD(settlement, maturity, last_interest, rate, pr, redemption, frequency, [basis])",
    ),
    ("OFFSET", "OFFSET(reference, rows, cols, [height], [width])"),
    ("OR", "OR(logical1, …)"),
    ("PDURATION", "PDURATION(rate, pv, fv)"),
    ("PEARSON", "PEARSON(array1, array2)"),
    ("PERCENTILE", "PERCENTILE(array, k)"),
    ("PERCENTILE.INC", "PERCENTILE.INC(array, k)"),
    ("PERCENTRANK", "PERCENTRANK(array, x, [significance])"),
    (
        "PERCENTRANK.INC",
        "PERCENTRANK.INC(array, x, [significance])",
    ),
    ("PERMUT", "PERMUT(n, k)"),
    ("PERMUTATIONA", "PERMUTATIONA(n, k)"),
    ("PI", "PI()"),
    ("PMT", "PMT(rate, nper, pv, [fv], [type])"),
    ("POISSON", "POISSON(x, mean, cumulative)"),
    ("POISSON.DIST", "POISSON.DIST(x, mean, cumulative)"),
    ("POWER", "POWER(number, power)"),
    ("PPMT", "PPMT(rate, per, nper, pv, [fv], [type])"),
    (
        "PRICE",
        "PRICE(settlement, maturity, rate, yld, redemption, frequency, [basis])",
    ),
    (
        "PRICEDISC",
        "PRICEDISC(settlement, maturity, discount, redemption, [basis])",
    ),
    (
        "PRICEMAT",
        "PRICEMAT(settlement, maturity, issue, rate, yld, [basis])",
    ),
    ("PROB", "PROB(x_range, prob_range, lower, [upper])"),
    ("PRODUCT", "PRODUCT(number1, …)"),
    ("PROPER", "PROPER(text)"),
    ("PV", "PV(rate, nper, pmt, [fv], [type])"),
    ("QUARTILE", "QUARTILE(array, quart)"),
    ("QUARTILE.INC", "QUARTILE.INC(array, quart)"),
    ("QUOTIENT", "QUOTIENT(numerator, denominator)"),
    ("RADIANS", "RADIANS(angle)"),
    ("RAND", "RAND()"),
    ("RANDBETWEEN", "RANDBETWEEN(bottom, top)"),
    ("RANK", "RANK(number, ref, [order])"),
    ("RANK.EQ", "RANK.EQ(number, ref, [order])"),
    ("RATE", "RATE(nper, pmt, pv, [fv], [type], [guess])"),
    (
        "RECEIVED",
        "RECEIVED(settlement, maturity, investment, discount, [basis])",
    ),
    ("REDUCE", "REDUCE(initial_value, array, lambda)"),
    ("REPLACE", "REPLACE(old, start, num_chars, new)"),
    (
        "REPLACEB",
        "REPLACEB(old_text, start_num, num_bytes, new_text)",
    ),
    ("REPT", "REPT(text, number_times)"),
    ("RIGHT", "RIGHT(text, [num_chars])"),
    ("RIGHTB", "RIGHTB(text, [num_bytes])"),
    ("ROMAN", "ROMAN(number, [form])"),
    ("ROUND", "ROUND(number, num_digits)"),
    ("ROUNDDOWN", "ROUNDDOWN(number, num_digits)"),
    ("ROUNDUP", "ROUNDUP(number, num_digits)"),
    ("ROW", "ROW([reference])"),
    ("ROWS", "ROWS(array)"),
    ("RRI", "RRI(nper, pv, fv)"),
    ("RSQ", "RSQ(known_y, known_x)"),
    ("SCAN", "SCAN(initial_value, array, lambda)"),
    ("SEARCH", "SEARCH(find_text, within_text, [start])"),
    ("SEARCHB", "SEARCHB(find_text, within_text, [start_num])"),
    ("SEC", "SEC(number)"),
    ("SECH", "SECH(number)"),
    ("SECOND", "SECOND(serial_number)"),
    ("SEQUENCE", "SEQUENCE(rows, [columns], [start], [step])"),
    ("SERIESSUM", "SERIESSUM(x, n, m, coefficients)"),
    ("SHEET", "SHEET([value])"),
    ("SHEETS", "SHEETS([reference])"),
    ("SIGN", "SIGN(number)"),
    ("SIN", "SIN(number)"),
    ("SINH", "SINH(number)"),
    ("SKEW", "SKEW(number1, …)"),
    ("SLN", "SLN(cost, salvage, life)"),
    ("SLOPE", "SLOPE(known_y, known_x)"),
    ("SMALL", "SMALL(array, k)"),
    ("SORT", "SORT(array, [sort_index], [sort_order], [by_col])"),
    ("SORTBY", "SORTBY(array, by_array, [sort_order])"),
    ("SQRT", "SQRT(number)"),
    ("SQRTPI", "SQRTPI(number)"),
    ("STANDARDIZE", "STANDARDIZE(x, mean, standard_dev)"),
    ("STDEV", "STDEV(number1, …)"),
    ("STDEV.P", "STDEV.P(number1, …)"),
    ("STDEV.S", "STDEV.S(number1, …)"),
    ("STDEVA", "STDEVA(value1, …)"),
    ("STDEVP", "STDEVP(number1, …)"),
    ("STDEVPA", "STDEVPA(value1, …)"),
    ("STEYX", "STEYX(known_y, known_x)"),
    ("SUBSTITUTE", "SUBSTITUTE(text, old, new, [instance])"),
    ("SUBTOTAL", "SUBTOTAL(function_num, ref1, …)"),
    ("SUM", "SUM(number1, …)"),
    ("SUMIF", "SUMIF(range, criteria, [sum_range])"),
    ("SUMIFS", "SUMIFS(sum_range, range1, criteria1, …)"),
    ("SUMPRODUCT", "SUMPRODUCT(array1, …)"),
    ("SUMSQ", "SUMSQ(number1, …)"),
    ("SUMX2MY2", "SUMX2MY2(array_x, array_y)"),
    ("SUMX2PY2", "SUMX2PY2(array_x, array_y)"),
    ("SUMXMY2", "SUMXMY2(array_x, array_y)"),
    (
        "SWITCH",
        "SWITCH(expression, value1, result1, …, [default])",
    ),
    ("SYD", "SYD(cost, salvage, life, per)"),
    ("T", "T(value)"),
    ("T.INV.2T", "T.INV.2T(probability, degrees_freedom)"),
    ("T.TEST", "T.TEST(array1, array2, tails, type)"),
    ("TAN", "TAN(number)"),
    ("TANH", "TANH(number)"),
    ("TBILLEQ", "TBILLEQ(settlement, maturity, discount)"),
    ("TBILLPRICE", "TBILLPRICE(settlement, maturity, discount)"),
    ("TBILLYIELD", "TBILLYIELD(settlement, maturity, pr)"),
    ("TDIST", "TDIST(x, degrees_freedom, tails)"),
    ("TEXT", "TEXT(value, format_code)"),
    ("TEXTJOIN", "TEXTJOIN(delimiter, ignore_empty, text1, …)"),
    ("TIME", "TIME(hour, minute, second)"),
    ("TIMEVALUE", "TIMEVALUE(time_text)"),
    ("TINV", "TINV(probability, degrees_freedom)"),
    ("TODAY", "TODAY()"),
    ("TRANSPOSE", "TRANSPOSE(array)"),
    ("TREND", "TREND(known_y, [known_x], [new_x], [const])"),
    ("TRIM", "TRIM(text)"),
    ("TRIMMEAN", "TRIMMEAN(array, percent)"),
    ("TRUE", "TRUE()"),
    ("TRUNC", "TRUNC(number, [num_digits])"),
    ("TTEST", "TTEST(array1, array2, tails, type)"),
    ("TYPE", "TYPE(value)"),
    ("UNICHAR", "UNICHAR(number)"),
    ("UNICODE", "UNICODE(text)"),
    ("UNIQUE", "UNIQUE(array, [by_col], [exactly_once])"),
    ("UPPER", "UPPER(text)"),
    ("USDOLLAR", "USDOLLAR(number, [decimals])"),
    ("VALUE", "VALUE(text)"),
    ("VAR", "VAR(number1, …)"),
    ("VAR.P", "VAR.P(number1, …)"),
    ("VAR.S", "VAR.S(number1, …)"),
    ("VARA", "VARA(value1, …)"),
    ("VARP", "VARP(number1, …)"),
    ("VARPA", "VARPA(value1, …)"),
    (
        "VDB",
        "VDB(cost, salvage, life, start_period, end_period, [factor], [no_switch])",
    ),
    ("VLOOKUP", "VLOOKUP(lookup, table, col, [exact])"),
    ("WEEKDAY", "WEEKDAY(serial_number, [type])"),
    ("WEEKNUM", "WEEKNUM(serial, [type])"),
    ("WEIBULL", "WEIBULL(x, alpha, beta, cumulative)"),
    ("WEIBULL.DIST", "WEIBULL.DIST(x, alpha, beta, cumulative)"),
    ("WORKDAY", "WORKDAY(start, days, [holidays])"),
    (
        "WORKDAY.INTL",
        "WORKDAY.INTL(start, days, [weekend], [holidays])",
    ),
    ("XIRR", "XIRR(values, dates, [guess])"),
    (
        "XLOOKUP",
        "XLOOKUP(lookup, lookup_array, return_array, [if_not_found], [match_mode], [search_mode])",
    ),
    (
        "XMATCH",
        "XMATCH(lookup, lookup_array, [match_mode], [search_mode])",
    ),
    ("XNPV", "XNPV(rate, values, dates)"),
    ("YEAR", "YEAR(serial_number)"),
    ("YEARFRAC", "YEARFRAC(start, end, [basis])"),
    (
        "YIELD",
        "YIELD(settlement, maturity, rate, pr, redemption, frequency, [basis])",
    ),
    (
        "YIELDDISC",
        "YIELDDISC(settlement, maturity, pr, redemption, [basis])",
    ),
    (
        "YIELDMAT",
        "YIELDMAT(settlement, maturity, issue, rate, pr, [basis])",
    ),
    ("Z.TEST", "Z.TEST(array, x, [sigma])"),
    ("ZTEST", "ZTEST(array, x, [sigma])"),
];

/// Dispatch a function call by (upper-cased) name.
pub fn call_function(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    match name {
        "SUM" => match flatten_numbers(ev, sheet, args) {
            Ok(ns) => Value::Number(ns.iter().sum()),
            Err(e) => Value::Error(e),
        },
        "AVERAGE" => match flatten_numbers(ev, sheet, args) {
            Ok(ns) if ns.is_empty() => Value::Error(ErrorValue::Div0),
            Ok(ns) => Value::Number(ns.iter().sum::<f64>() / ns.len() as f64),
            Err(e) => Value::Error(e),
        },
        "COUNT" => match flatten_numbers(ev, sheet, args) {
            Ok(ns) => Value::Number(ns.len() as f64),
            Err(e) => Value::Error(e),
        },
        "COUNTA" => eval_counta(ev, sheet, args),
        "MIN" => reduce(ev, sheet, args, f64::min),
        "MAX" => reduce(ev, sheet, args, f64::max),
        "IF" => eval_if(ev, sheet, args),
        "IFERROR" => eval_iferror(ev, sheet, args),
        "AND" => eval_and_or(ev, sheet, args, true),
        "OR" => eval_and_or(ev, sheet, args, false),
        "NOT" => eval_not(ev, sheet, args),
        "COUNTIF" => eval_countif(ev, sheet, args),
        "SUMIF" => eval_sumif(ev, sheet, args),
        "AVERAGEIF" => eval_averageif(ev, sheet, args),
        "ABS" => scalar(ev, sheet, args, f64::abs),
        // Trigonometry and friends. The spec defines these as the ordinary
        // mathematical functions, so they delegate to the standard library;
        // what needs care is the domain errors below, where Excel answers
        // #NUM! rather than the IEEE NaN Rust would hand back.
        "SIN" => scalar(ev, sheet, args, f64::sin),
        "COS" => scalar(ev, sheet, args, f64::cos),
        "TAN" => scalar(ev, sheet, args, f64::tan),
        "SINH" => scalar(ev, sheet, args, f64::sinh),
        "COSH" => scalar(ev, sheet, args, f64::cosh),
        "TANH" => scalar(ev, sheet, args, f64::tanh),
        "ATAN" => scalar(ev, sheet, args, f64::atan),
        "DEGREES" => scalar(ev, sheet, args, f64::to_degrees),
        "RADIANS" => scalar(ev, sheet, args, f64::to_radians),
        "EXP" => scalar(ev, sheet, args, f64::exp),
        // Reciprocal trig: a zero denominator is #DIV/0!, not infinity.
        "COT" => checked(ev, sheet, args, |n| {
            finite_or(1.0 / n.tan(), ErrorValue::Div0)
        }),
        "COTH" => checked(ev, sheet, args, |n| {
            finite_or(1.0 / n.tanh(), ErrorValue::Div0)
        }),
        "CSC" => checked(ev, sheet, args, |n| {
            finite_or(1.0 / n.sin(), ErrorValue::Div0)
        }),
        "CSCH" => checked(ev, sheet, args, |n| {
            finite_or(1.0 / n.sinh(), ErrorValue::Div0)
        }),
        "SEC" => checked(ev, sheet, args, |n| {
            finite_or(1.0 / n.cos(), ErrorValue::Div0)
        }),
        "SECH" => checked(ev, sheet, args, |n| {
            finite_or(1.0 / n.cosh(), ErrorValue::Div0)
        }),
        // Domain-restricted: outside the domain the answer is #NUM!.
        "ASIN" => checked(ev, sheet, args, |n| domain(n.asin())),
        "ACOS" => checked(ev, sheet, args, |n| domain(n.acos())),
        "ACOSH" => checked(ev, sheet, args, |n| domain(n.acosh())),
        "ASINH" => scalar(ev, sheet, args, f64::asinh),
        "ATANH" => checked(ev, sheet, args, |n| domain(n.atanh())),
        "LN" => checked(ev, sheet, args, |n| domain(n.ln())),
        "LOG10" => checked(ev, sheet, args, |n| domain(n.log10())),
        "SQRTPI" => checked(ev, sheet, args, |n| {
            domain((n * std::f64::consts::PI).sqrt())
        }),
        "PI" => {
            if args.is_empty() {
                Value::Number(std::f64::consts::PI)
            } else {
                Value::Error(ErrorValue::Value)
            }
        }
        // ATAN2's arguments are (x, y) in OOXML — the reverse of the atan2(y, x)
        // convention every maths library uses. Passing them straight through
        // would silently reflect every angle about the diagonal.
        "ATAN2" => eval_atan2(ev, sheet, args),
        "LOG" => eval_log(ev, sheet, args),
        "EVEN" => scalar(ev, sheet, args, |n| round_away_to(n, 2.0)),
        "ODD" => scalar(ev, sheet, args, eval_odd),
        "QUOTIENT" => eval_quotient(ev, sheet, args),
        "MROUND" => eval_mround(ev, sheet, args),
        "FACT" => checked(ev, sheet, args, |n| factorial(n.trunc())),
        "FACTDOUBLE" => checked(ev, sheet, args, factorial_double),
        "COMBIN" => eval_combin(ev, sheet, args, false),
        "COMBINA" => eval_combin(ev, sheet, args, true),
        "PERMUT" => eval_permut(ev, sheet, args, false),
        "PERMUTATIONA" => eval_permut(ev, sheet, args, true),
        "GCD" => eval_gcd_lcm(ev, sheet, args, true),
        "LCM" => eval_gcd_lcm(ev, sheet, args, false),
        "MULTINOMIAL" => eval_multinomial(ev, sheet, args),
        "SUMSQ" => match flatten_numbers(ev, sheet, args) {
            Ok(ns) => Value::Number(ns.iter().map(|n| n * n).sum()),
            Err(e) => Value::Error(e),
        },
        "SERIESSUM" => eval_seriessum(ev, sheet, args),
        "INT" => scalar(ev, sheet, args, f64::floor),
        "SQRT" => eval_sqrt(ev, sheet, args),
        "MOD" => eval_mod(ev, sheet, args),
        "POWER" => eval_power(ev, sheet, args),
        "ROUND" => eval_round(ev, sheet, args),
        "CONCATENATE" | "CONCAT" => eval_concat(ev, sheet, args),
        "LEN" => eval_len(ev, sheet, args),
        "LEFT" => eval_left(ev, sheet, args),
        "LENB" | "LEFTB" | "RIGHTB" | "MIDB" | "FINDB" | "SEARCHB" | "REPLACEB" => {
            eval_text_bytes(ev, sheet, name, args)
        }
        "ASC" => eval_width_convert(ev, sheet, args, false),
        "JIS" => eval_width_convert(ev, sheet, args, true),
        "RIGHT" => eval_right(ev, sheet, args),
        "MID" => eval_mid(ev, sheet, args),
        "UPPER" => text_op(ev, sheet, args, |s| s.to_uppercase()),
        "LOWER" => text_op(ev, sheet, args, |s| s.to_lowercase()),
        "TRIM" => text_op(ev, sheet, args, trim_excel),
        "PRODUCT" => eval_product(ev, sheet, args),
        "ROUNDUP" => eval_round_dir(ev, sheet, args, RoundDir::Up),
        "ROUNDDOWN" => eval_round_dir(ev, sheet, args, RoundDir::Down),
        "TRUNC" => eval_trunc(ev, sheet, args),
        "CEILING" => eval_ceiling_floor(ev, sheet, args, true),
        "FLOOR" => eval_ceiling_floor(ev, sheet, args, false),
        "SIGN" => eval_sign(ev, sheet, args),
        "VLOOKUP" => eval_vlookup(ev, sheet, args, true),
        "HLOOKUP" => eval_vlookup(ev, sheet, args, false),
        "INDEX" => eval_index(ev, sheet, args),
        "MATCH" => eval_match(ev, sheet, args),
        "CHOOSE" => eval_choose(ev, sheet, args),
        "SUBSTITUTE" => eval_substitute(ev, sheet, args),
        "REPLACE" => eval_replace(ev, sheet, args),
        "FIND" => eval_find_search(ev, sheet, args, true),
        "SEARCH" => eval_find_search(ev, sheet, args, false),
        "VALUE" => eval_value(ev, sheet, args),
        "PROPER" => text_op(ev, sheet, args, proper_case),
        "REPT" => eval_rept(ev, sheet, args),
        "EXACT" => eval_exact(ev, sheet, args),
        "TIME" => eval_time(ev, sheet, args),
        "HOUR" => eval_time_part(ev, sheet, args, 3600.0),
        "MINUTE" => eval_time_part(ev, sheet, args, 60.0),
        "SECOND" => eval_time_part(ev, sheet, args, 1.0),
        "DAYS" => match pair_of_numbers(ev, sheet, args) {
            Ok([end, start]) => Value::Number(end.trunc() - start.trunc()),
            Err(e) => e,
        },
        "DAYS360" => eval_days360(ev, sheet, args),
        "DATEDIF" => eval_datedif(ev, sheet, args),
        "WEEKNUM" => eval_weeknum(ev, sheet, args),
        "ISOWEEKNUM" => eval_isoweeknum(ev, sheet, args),
        "YEARFRAC" => eval_yearfrac(ev, sheet, args),
        "NETWORKDAYS" => eval_workdays(ev, sheet, args, false),
        "NETWORKDAYS.INTL" => eval_workdays_intl(ev, sheet, args, false),
        "WORKDAY.INTL" => eval_workdays_intl(ev, sheet, args, true),
        "DATEVALUE" => eval_datevalue(ev, sheet, args, false),
        "TIMEVALUE" => eval_datevalue(ev, sheet, args, true),
        "TODAY" | "NOW" | "RAND" | "RANDBETWEEN" => eval_volatile(ev, sheet, name, args),
        "WORKDAY" => eval_workdays(ev, sheet, args, true),
        "ADDRESS" => eval_address(ev, sheet, args),
        "INDIRECT" => eval_indirect(ev, sheet, args),
        "OFFSET" => eval_offset(ev, sheet, args),
        "AREAS" => eval_areas(args),
        "LOOKUP" => eval_lookup(ev, sheet, args),
        // HYPERLINK displays its friendly name and otherwise evaluates to the
        // link text; the navigation is the host's job, not the engine's.
        "HYPERLINK" => match args {
            [link] => ev.eval_expr(sheet, link),
            [link, friendly] => {
                let value = ev.eval_expr(sheet, friendly);
                if matches!(value, Value::Empty) {
                    ev.eval_expr(sheet, link)
                } else {
                    value
                }
            }
            _ => Value::Error(ErrorValue::Value),
        },
        "CHAR" => eval_char(ev, sheet, args, false),
        "UNICHAR" => eval_char(ev, sheet, args, true),
        "CODE" => eval_code(ev, sheet, args, false),
        "UNICODE" => eval_code(ev, sheet, args, true),
        "CLEAN" => eval_clean(ev, sheet, args),
        // `T` passes text through and answers empty for everything else — it
        // does *not* convert, which is the difference from TEXT.
        "T" => match args {
            [arg] => match ev.eval_expr(sheet, arg) {
                Value::Text(t) => Value::Text(t),
                Value::Error(e) => Value::Error(e),
                _ => Value::Text(String::new()),
            },
            _ => Value::Error(ErrorValue::Value),
        },
        "FIXED" => eval_fixed(ev, sheet, args),
        // `USDOLLAR` is the legacy alias of `DOLLAR`, kept because old files
        // still contain it.
        "DOLLAR" | "USDOLLAR" => eval_dollar(ev, sheet, args),
        "BAHTTEXT" => eval_bahttext(ev, sheet, args),
        "BESSELI" | "BESSELJ" | "BESSELK" | "BESSELY" => eval_bessel(ev, sheet, name, args),
        "NUMBERVALUE" => eval_numbervalue(ev, sheet, args),
        // Descriptive statistics over the flattened numeric arguments.
        "AVEDEV" => stat_over(ev, sheet, args, |ns| {
            let mean = ns.iter().sum::<f64>() / ns.len() as f64;
            Some(ns.iter().map(|n| (n - mean).abs()).sum::<f64>() / ns.len() as f64)
        }),
        "DEVSQ" => stat_over(ev, sheet, args, |ns| {
            let mean = ns.iter().sum::<f64>() / ns.len() as f64;
            Some(ns.iter().map(|n| (n - mean).powi(2)).sum())
        }),
        "GEOMEAN" => stat_over(ev, sheet, args, |ns| {
            // Any non-positive value makes the geometric mean undefined, and
            // the log-sum below would silently yield NaN instead of saying so.
            if ns.iter().any(|n| *n <= 0.0) {
                return None;
            }
            Some((ns.iter().map(|n| n.ln()).sum::<f64>() / ns.len() as f64).exp())
        }),
        "HARMEAN" => stat_over(ev, sheet, args, |ns| {
            if ns.iter().any(|n| *n <= 0.0) {
                return None;
            }
            Some(ns.len() as f64 / ns.iter().map(|n| 1.0 / n).sum::<f64>())
        }),
        "MODE" | "MODE.SNGL" => stat_over(ev, sheet, args, mode_of),
        "SKEW" => stat_over(ev, sheet, args, skew_of),
        "KURT" => stat_over(ev, sheet, args, kurt_of),
        "VAR" | "VAR.S" => stat_over(ev, sheet, args, |ns| variance(ns, true)),
        "VARP" | "VAR.P" => stat_over(ev, sheet, args, |ns| variance(ns, false)),
        "PERCENTILE" | "PERCENTILE.INC" => eval_percentile(ev, sheet, args, false),
        "QUARTILE" | "QUARTILE.INC" => eval_percentile(ev, sheet, args, true),
        "PERCENTRANK" | "PERCENTRANK.INC" => eval_percentrank(ev, sheet, args),
        "TRIMMEAN" => eval_trimmean(ev, sheet, args),
        "COUNTBLANK" => eval_countblank(ev, sheet, args),
        "STANDARDIZE" => eval_standardize(ev, sheet, args),
        // Paired-sample statistics: two ranges of equal length.
        "CORREL" | "PEARSON" => paired(ev, sheet, args, correlation),
        "RSQ" => paired(ev, sheet, args, |xs, ys| correlation(xs, ys).map(|r| r * r)),
        "COVAR" | "COVARIANCE.P" => paired(ev, sheet, args, |xs, ys| {
            let (mx, my) = (mean(xs), mean(ys));
            Some(
                xs.iter()
                    .zip(ys)
                    .map(|(x, y)| (x - mx) * (y - my))
                    .sum::<f64>()
                    / xs.len() as f64,
            )
        }),
        // Note the argument order: SLOPE and INTERCEPT take y *before* x, so
        // the regression is of the first range on the second.
        "SLOPE" => paired(ev, sheet, args, slope),
        "INTERCEPT" => paired(ev, sheet, args, |ys, xs| {
            slope(ys, xs).map(|m| mean(ys) - m * mean(xs))
        }),
        "STEYX" => paired(ev, sheet, args, steyx),
        "FORECAST" => eval_forecast(ev, sheet, args),
        // Distributions and transforms.
        "FISHER" => checked(ev, sheet, args, |x| {
            if x <= -1.0 || x >= 1.0 {
                Value::Error(ErrorValue::Num)
            } else {
                Value::Number(0.5 * ((1.0 + x) / (1.0 - x)).ln())
            }
        }),
        "FISHERINV" => scalar(ev, sheet, args, |y| {
            let e = (2.0 * y).exp();
            (e - 1.0) / (e + 1.0)
        }),
        "GAMMALN" => checked(ev, sheet, args, |x| {
            if x <= 0.0 {
                Value::Error(ErrorValue::Num)
            } else {
                Value::Number(ln_gamma(x))
            }
        }),
        "NORMSDIST" => scalar(ev, sheet, args, standard_normal_cdf),
        // 2010 made `cumulative` a *required* argument, so this is not an
        // alias of `NORMSDIST`: `scalar` ignores trailing arguments, and
        // aliasing would have returned the CDF where the density was asked
        // for — a wrong number rather than a visible error.
        "NORM.S.DIST" => eval_norm_s_dist(ev, sheet, args),
        "NORMSINV" | "NORM.S.INV" => checked(ev, sheet, args, |p| {
            if p <= 0.0 || p >= 1.0 {
                Value::Error(ErrorValue::Num)
            } else {
                Value::Number(normal_quantile(p))
            }
        }),
        "NORMDIST" | "NORM.DIST" => eval_normdist(ev, sheet, args),
        "NORMINV" | "NORM.INV" => eval_norminv(ev, sheet, args),
        "EXPONDIST" | "EXPON.DIST" => eval_expondist(ev, sheet, args),
        "POISSON" | "POISSON.DIST" => eval_poisson(ev, sheet, args),
        "BINOMDIST" | "BINOM.DIST" => eval_binomdist(ev, sheet, args),
        // The `A` variants count text as 0 and logicals as 0/1, where the plain
        // forms skip non-numbers entirely. That difference is the only reason
        // both exist, so they share nothing but the reduction.
        "AVERAGEA" => stat_over_a(ev, sheet, args, |ns| Some(mean(ns))),
        "MAXA" => stat_over_a(ev, sheet, args, |ns| ns.iter().copied().reduce(f64::max)),
        "MINA" => stat_over_a(ev, sheet, args, |ns| ns.iter().copied().reduce(f64::min)),
        "VARA" => stat_over_a(ev, sheet, args, |ns| variance(ns, true)),
        "VARPA" => stat_over_a(ev, sheet, args, |ns| variance(ns, false)),
        "STDEVA" => stat_over_a(ev, sheet, args, |ns| variance(ns, true).map(f64::sqrt)),
        "STDEVPA" => stat_over_a(ev, sheet, args, |ns| variance(ns, false).map(f64::sqrt)),
        "LOGNORMDIST" => eval_lognormdist(ev, sheet, args),
        // Likewise gained a required `cumulative` flag in 2010.
        "LOGNORM.DIST" => eval_lognorm_dist(ev, sheet, args),
        "LOGINV" | "LOGNORM.INV" => eval_loginv(ev, sheet, args),
        "WEIBULL" | "WEIBULL.DIST" => eval_weibull(ev, sheet, args),
        "NEGBINOMDIST" => eval_negbinomdist(ev, sheet, args),
        "HYPGEOMDIST" => eval_hypgeomdist(ev, sheet, args),
        "CRITBINOM" => eval_critbinom(ev, sheet, args),
        "CONFIDENCE" | "CONFIDENCE.NORM" => eval_confidence(ev, sheet, args),
        // Base conversion. Each pair is (from, to) radix; the negative handling
        // lives in the helpers because it is the part that differs.
        "BIN2DEC" => base_to_dec(ev, sheet, args, 2),
        "OCT2DEC" => base_to_dec(ev, sheet, args, 8),
        "HEX2DEC" => base_to_dec(ev, sheet, args, 16),
        "DEC2BIN" => dec_to_base(ev, sheet, args, 2),
        "DEC2OCT" => dec_to_base(ev, sheet, args, 8),
        "DEC2HEX" => dec_to_base(ev, sheet, args, 16),
        "BIN2OCT" => base_to_base(ev, sheet, args, 2, 8),
        "BIN2HEX" => base_to_base(ev, sheet, args, 2, 16),
        "OCT2BIN" => base_to_base(ev, sheet, args, 8, 2),
        "OCT2HEX" => base_to_base(ev, sheet, args, 8, 16),
        "HEX2BIN" => base_to_base(ev, sheet, args, 16, 2),
        "HEX2OCT" => base_to_base(ev, sheet, args, 16, 8),
        "BITAND" => bitwise(ev, sheet, args, |a, b| a & b),
        "BITOR" => bitwise(ev, sheet, args, |a, b| a | b),
        "BITXOR" => bitwise(ev, sheet, args, |a, b| a ^ b),
        "BITLSHIFT" => bit_shift(ev, sheet, args, true),
        "BITRSHIFT" => bit_shift(ev, sheet, args, false),
        "DELTA" => eval_delta(ev, sheet, args, true),
        "GESTEP" => eval_delta(ev, sheet, args, false),
        "GETPIVOTDATA" => eval_getpivotdata(ev, sheet, args),
        "ERF" => eval_erf(ev, sheet, args),
        "ERFC" => scalar(ev, sheet, args, |x| 1.0 - erf(x)),
        // The annuity family. All five are the same equation rearranged, so
        // they share `annuity_factor` rather than repeating the algebra.
        "PV" => eval_pv(ev, sheet, args),
        "FV" => eval_fv(ev, sheet, args),
        "PMT" => eval_pmt(ev, sheet, args),
        "NPER" => eval_nper(ev, sheet, args),
        "RATE" => eval_rate(ev, sheet, args),
        "IPMT" => eval_ipmt(ev, sheet, args, true),
        "PPMT" => eval_ipmt(ev, sheet, args, false),
        "ISPMT" => eval_ispmt(ev, sheet, args),
        "NPV" => eval_npv(ev, sheet, args),
        "IRR" => eval_irr(ev, sheet, args),
        "MIRR" => eval_mirr(ev, sheet, args),
        "XNPV" => eval_xnpv(ev, sheet, args),
        "XIRR" => eval_xirr(ev, sheet, args),
        "FVSCHEDULE" => eval_fvschedule(ev, sheet, args),
        // Depreciation.
        "SLN" => eval_sln(ev, sheet, args),
        "SYD" => eval_syd(ev, sheet, args),
        "DB" => eval_db(ev, sheet, args),
        "DDB" => eval_ddb(ev, sheet, args),
        "VDB" => eval_vdb(ev, sheet, args),
        "ACCRINT" => eval_accrint(ev, sheet, args),
        "AMORLINC" => eval_amor(ev, sheet, args, false),
        "AMORDEGRC" => eval_amor(ev, sheet, args, true),
        // Rate conversions.
        "EFFECT" => eval_effect(ev, sheet, args, true),
        "NOMINAL" => eval_effect(ev, sheet, args, false),
        "RRI" => eval_rri(ev, sheet, args),
        "PDURATION" => eval_pduration(ev, sheet, args),
        "DOLLARDE" => eval_dollar_frac(ev, sheet, args, true),
        "DOLLARFR" => eval_dollar_frac(ev, sheet, args, false),
        // Complex numbers. They travel as *text* ("3+4i"), which is why every
        // one of these parses and re-formats rather than taking a pair of
        // numbers: the suffix (i or j) is part of the value and must survive.
        "COMPLEX" => eval_complex(ev, sheet, args),
        "IMREAL" => complex_part(ev, sheet, args, |c| c.0),
        "IMAGINARY" => complex_part(ev, sheet, args, |c| c.1),
        "IMABS" => complex_part(ev, sheet, args, |c| c.0.hypot(c.1)),
        "IMARGUMENT" => complex_part(ev, sheet, args, |c| c.1.atan2(c.0)),
        "IMCONJUGATE" => complex_map(ev, sheet, args, |c| (c.0, -c.1)),
        "IMSUM" => complex_fold(ev, sheet, args, |a, b| (a.0 + b.0, a.1 + b.1), (0.0, 0.0)),
        "IMPRODUCT" => complex_fold(
            ev,
            sheet,
            args,
            |a, b| (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0),
            (1.0, 0.0),
        ),
        "IMSUB" => complex_pair(ev, sheet, args, |a, b| Some((a.0 - b.0, a.1 - b.1))),
        "IMDIV" => complex_pair(ev, sheet, args, |a, b| {
            let d = b.0 * b.0 + b.1 * b.1;
            (d != 0.0).then(|| ((a.0 * b.0 + a.1 * b.1) / d, (a.1 * b.0 - a.0 * b.1) / d))
        }),
        "IMEXP" => complex_map(ev, sheet, args, |c| {
            let e = c.0.exp();
            (e * c.1.cos(), e * c.1.sin())
        }),
        "IMLN" => complex_map(ev, sheet, args, |c| (c.0.hypot(c.1).ln(), c.1.atan2(c.0))),
        "IMLOG10" => complex_map(ev, sheet, args, |c| {
            let ln10 = std::f64::consts::LN_10;
            (c.0.hypot(c.1).ln() / ln10, c.1.atan2(c.0) / ln10)
        }),
        "IMLOG2" => complex_map(ev, sheet, args, |c| {
            let ln2 = std::f64::consts::LN_2;
            (c.0.hypot(c.1).ln() / ln2, c.1.atan2(c.0) / ln2)
        }),
        "IMSQRT" => complex_map(ev, sheet, args, |c| {
            let r = c.0.hypot(c.1).sqrt();
            let t = c.1.atan2(c.0) / 2.0;
            (r * t.cos(), r * t.sin())
        }),
        "IMPOWER" => eval_impower(ev, sheet, args),
        "IMSIN" => complex_map(ev, sheet, args, |c| {
            (c.0.sin() * c.1.cosh(), c.0.cos() * c.1.sinh())
        }),
        "IMCOS" => complex_map(ev, sheet, args, |c| {
            (c.0.cos() * c.1.cosh(), -c.0.sin() * c.1.sinh())
        }),
        "IMSINH" => complex_map(ev, sheet, args, |c| {
            (c.0.sinh() * c.1.cos(), c.0.cosh() * c.1.sin())
        }),
        "IMCOSH" => complex_map(ev, sheet, args, |c| {
            (c.0.cosh() * c.1.cos(), c.0.sinh() * c.1.sin())
        }),
        "IMTAN" => complex_pair_self(ev, sheet, args, |c| {
            let (sr, si) = (c.0.sin() * c.1.cosh(), c.0.cos() * c.1.sinh());
            let (cr, ci) = (c.0.cos() * c.1.cosh(), -c.0.sin() * c.1.sinh());
            let d = cr * cr + ci * ci;
            (d != 0.0).then(|| ((sr * cr + si * ci) / d, (si * cr - sr * ci) / d))
        }),
        // The chi-square / t / F family. All are incomplete gamma or beta
        // underneath, so they share those two rather than repeating series.
        "CHIDIST" | "CHISQ.DIST.RT" => eval_chidist(ev, sheet, args),
        "CHIINV" | "CHISQ.INV.RT" => eval_chiinv(ev, sheet, args),
        "TDIST" => eval_tdist(ev, sheet, args),
        "TINV" | "T.INV.2T" => eval_tinv(ev, sheet, args),
        "FDIST" | "F.DIST.RT" => eval_fdist(ev, sheet, args),
        "FINV" | "F.INV.RT" => eval_finv(ev, sheet, args),
        "GAMMADIST" | "GAMMA.DIST" => eval_gammadist(ev, sheet, args),
        "GAMMAINV" | "GAMMA.INV" => eval_gammainv(ev, sheet, args),
        "BETADIST" => eval_betadist(ev, sheet, args),
        "BETAINV" | "BETA.INV" => eval_betainv(ev, sheet, args),
        "ZTEST" | "Z.TEST" => eval_ztest(ev, sheet, args),
        "TTEST" | "T.TEST" => eval_ttest(ev, sheet, args),
        "FTEST" | "F.TEST" => eval_ftest(ev, sheet, args),
        "CHITEST" | "CHISQ.TEST" => eval_chitest(ev, sheet, args),
        "PROB" => eval_prob(ev, sheet, args),
        "SUBTOTAL" => eval_subtotal(ev, sheet, args),
        "SUMX2MY2" => paired(ev, sheet, args, |xs, ys| {
            Some(xs.iter().zip(ys).map(|(x, y)| x * x - y * y).sum())
        }),
        "SUMX2PY2" => paired(ev, sheet, args, |xs, ys| {
            Some(xs.iter().zip(ys).map(|(x, y)| x * x + y * y).sum())
        }),
        "SUMXMY2" => paired(ev, sheet, args, |xs, ys| {
            Some(xs.iter().zip(ys).map(|(x, y)| (x - y).powi(2)).sum())
        }),
        "ROMAN" => eval_roman(ev, sheet, args),
        // Both round away from zero to a multiple, differing only in how they
        // treat a negative number — which is the whole reason two names exist.
        "ISO.CEILING" => eval_ceiling_variant(ev, sheet, args, true),
        "ECMA.CEILING" => eval_ceiling_variant(ev, sheet, args, false),
        "CUMIPMT" => eval_cumulative(ev, sheet, args, true),
        "CUMPRINC" => eval_cumulative(ev, sheet, args, false),
        "DISC" => eval_disc(ev, sheet, args),
        "PRICE" | "YIELD" | "DURATION" | "MDURATION" => eval_bond(ev, sheet, name, args),
        "ODDFPRICE" | "ODDFYIELD" | "ODDLPRICE" | "ODDLYIELD" => {
            eval_odd_bond(ev, sheet, name, args)
        }
        "MDETERM" => eval_mdeterm(ev, sheet, args),
        // Both are intercepted by the evaluator, which is the only place with
        // a scope to bind into; these arms exist so the catalog and the
        // dispatch table stay in agreement.
        "LET" | "LAMBDA" => Value::Error(ErrorValue::Value),
        "MAP" | "REDUCE" | "SCAN" | "BYROW" | "BYCOL" | "MAKEARRAY" | "ISOMITTED" => {
            eval_lambda_helper(ev, sheet, name, args)
        }
        "LINEST" | "LOGEST" | "TREND" | "GROWTH" => eval_regression(ev, sheet, name, args),
        "TRANSPOSE" | "MMULT" | "MINVERSE" | "FREQUENCY" => eval_matrix(ev, sheet, name, args),
        "ACCRINTM" | "PRICEDISC" | "YIELDDISC" | "PRICEMAT" | "YIELDMAT" => {
            eval_bond_simple(ev, sheet, name, args)
        }
        "COUPDAYBS" | "COUPDAYS" | "COUPDAYSNC" | "COUPNCD" | "COUPNUM" | "COUPPCD" => {
            eval_coupon(ev, sheet, name, args)
        }
        "INTRATE" => eval_intrate(ev, sheet, args, false),
        "RECEIVED" => eval_intrate(ev, sheet, args, true),
        "TBILLPRICE" => eval_tbill(ev, sheet, args, 0),
        "TBILLYIELD" => eval_tbill(ev, sheet, args, 1),
        "TBILLEQ" => eval_tbill(ev, sheet, args, 2),
        "DATE" => eval_date(ev, sheet, args),
        "YEAR" => eval_date_part(ev, sheet, args, DatePart::Year),
        "MONTH" => eval_date_part(ev, sheet, args, DatePart::Month),
        "DAY" => eval_date_part(ev, sheet, args, DatePart::Day),
        "WEEKDAY" => eval_weekday(ev, sheet, args),
        "EDATE" => eval_edate(ev, sheet, args, false),
        "EOMONTH" => eval_edate(ev, sheet, args, true),
        // --- Logical / info (M6-2) ---
        "IFS" => eval_ifs(ev, sheet, args),
        "SWITCH" => eval_switch(ev, sheet, args),
        "IFNA" => eval_ifna(ev, sheet, args),
        "NA" => Value::Error(ErrorValue::Na),
        // The literal-valued logicals. Both take no arguments; `TRUE(1)` is an
        // error rather than being tolerated, because a stray argument almost
        // always means the author meant something else.
        "TRUE" => nullary(args, Value::Bool(true)),
        "FALSE" => nullary(args, Value::Bool(false)),
        // `N` coerces to a number the way a spreadsheet does: text becomes 0
        // rather than an error, which is the whole reason the function exists.
        "N" => eval_n(ev, sheet, args),
        "TYPE" => eval_type(ev, sheet, args),
        "ERROR.TYPE" => eval_error_type(ev, sheet, args),
        // A reference is not a distinct value here — the evaluator resolves one
        // to its contents before a function sees it — so ISREF answers by
        // inspecting the *expression*, not the value it produced.
        "ISREF" => eval_is_ref(args),
        "CELL" => eval_cell_info(ev, sheet, args),
        "CONVERT" => eval_convert(ev, sheet, args),
        "INFO" => eval_info(ev, sheet, args),
        "ISFORMULA" => eval_is_formula(ev, sheet, args),
        // Both are 1-based over the workbook's sheet order.
        "SHEET" => match args {
            // Without an argument, the sheet the formula is on.
            [] => Value::Number(sheet as f64 + 1.0),
            [Expr::Reference(r)] => match r.sheet.as_deref() {
                Some(name) => match sheet_index_by_name(ev, name) {
                    Some(i) => Value::Number(i as f64 + 1.0),
                    None => Value::Error(ErrorValue::Na),
                },
                None => Value::Number(sheet as f64 + 1.0),
            },
            _ => Value::Error(ErrorValue::Value),
        },
        "SHEETS" => match args {
            [] => Value::Number(ev.workbook().sheets.len() as f64),
            _ => Value::Error(ErrorValue::Value),
        },
        "ISBLANK" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Empty)),
        "ISNUMBER" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Number(_))),
        "ISTEXT" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Text(_))),
        "ISNONTEXT" => is_predicate(ev, sheet, args, |v| !matches!(v, Value::Text(_))),
        "ISLOGICAL" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Bool(_))),
        "ISERROR" => is_predicate(ev, sheet, args, |v| matches!(v, Value::Error(_))),
        "ISERR" => is_predicate(
            ev,
            sheet,
            args,
            |v| matches!(v, Value::Error(e) if *e != ErrorValue::Na),
        ),
        "ISNA" => is_predicate(ev, sheet, args, |v| {
            matches!(v, Value::Error(ErrorValue::Na))
        }),
        "ISEVEN" => eval_parity(ev, sheet, args, true),
        "ISODD" => eval_parity(ev, sheet, args, false),
        // --- Statistics (M6-2) ---
        "MEDIAN" => eval_median(ev, sheet, args),
        "LARGE" => eval_large_small(ev, sheet, args, true),
        "SMALL" => eval_large_small(ev, sheet, args, false),
        "RANK" | "RANK.EQ" => eval_rank(ev, sheet, args),
        "STDEV" | "STDEV.S" => eval_stdev(ev, sheet, args, true),
        "STDEVP" | "STDEV.P" => eval_stdev(ev, sheet, args, false),
        "SUMPRODUCT" => eval_sumproduct(ev, sheet, args),
        // --- Multi-criteria aggregates (M6-2) ---
        "SUMIFS" => eval_ifs_aggregate(ev, sheet, args, IfsKind::Sum),
        "MAXIFS" => eval_ifs_aggregate(ev, sheet, args, IfsKind::Max),
        "MINIFS" => eval_ifs_aggregate(ev, sheet, args, IfsKind::Min),
        "XLOOKUP" | "XMATCH" | "FILTER" | "UNIQUE" | "SORT" | "SORTBY" | "SEQUENCE" => {
            eval_dynamic(ev, sheet, name, args)
        }
        "DSUM" | "DAVERAGE" | "DCOUNT" | "DCOUNTA" | "DMAX" | "DMIN" | "DGET" | "DPRODUCT"
        | "DSTDEV" | "DSTDEVP" | "DVAR" | "DVARP" => eval_database(ev, sheet, name, args),
        "AVERAGEIFS" => eval_ifs_aggregate(ev, sheet, args, IfsKind::Average),
        "COUNTIFS" => eval_countifs(ev, sheet, args),
        // --- Shape / text (M6-2) ---
        "ROWS" => eval_dim(ev, sheet, args, true),
        "COLUMNS" => eval_dim(ev, sheet, args, false),
        "ROW" => eval_row_col(ev, args, true),
        "COLUMN" => eval_row_col(ev, args, false),
        "TEXT" => eval_text(ev, sheet, args),
        "TEXTJOIN" => eval_textjoin(ev, sheet, args),
        _ => Value::Error(ErrorValue::Name),
    }
}

pub(crate) fn reduce(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(f64, f64) -> f64,
) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) if ns.is_empty() => Value::Number(0.0),
        Ok(ns) => Value::Number(ns.into_iter().reduce(f).unwrap_or(0.0)),
        Err(e) => Value::Error(e),
    }
}

pub(crate) fn scalar(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(f64) -> f64,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => Value::Number(f(n)),
        Err(e) => Value::Error(e),
    }
}

pub(crate) fn eval_if(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, &args[0]).as_bool() {
        Ok(true) => ev.eval_expr(sheet, &args[1]),
        Ok(false) => {
            if args.len() == 3 {
                ev.eval_expr(sheet, &args[2])
            } else {
                Value::Bool(false)
            }
        }
        Err(e) => Value::Error(e),
    }
}

pub(crate) fn eval_iferror(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = ev.eval_expr(sheet, &args[0]);
    if value.as_error().is_some() {
        ev.eval_expr(sheet, &args[1])
    } else {
        value
    }
}

/// `AND`/`OR`. `require_all` true means `AND` (every truthy), false means `OR`.
pub(crate) fn eval_and_or(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    require_all: bool,
) -> Value {
    let mut any = false;
    let mut acc = require_all;
    for arg in args {
        for value in flatten_values(ev, sheet, arg) {
            // Ignore blanks (matches Excel's treatment of empty cells in ranges).
            if matches!(value, Value::Empty) {
                continue;
            }
            let b = match value.as_bool() {
                Ok(b) => b,
                Err(e) => return Value::Error(e),
            };
            any = true;
            if require_all {
                acc = acc && b;
            } else {
                acc = acc || b;
            }
        }
    }
    if !any {
        return Value::Error(ErrorValue::Value);
    }
    Value::Bool(acc)
}

pub(crate) fn eval_not(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, &args[0]).as_bool() {
        Ok(b) => Value::Bool(!b),
        Err(e) => Value::Error(e),
    }
}

pub(crate) fn eval_counta(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let mut count = 0u64;
    for arg in args {
        for value in flatten_values(ev, sheet, arg) {
            if !matches!(value, Value::Empty) {
                count += 1;
            }
        }
    }
    Value::Number(count as f64)
}

pub(crate) fn eval_sqrt(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) if n < 0.0 => Value::Error(ErrorValue::Num),
        Ok(n) => Value::Number(n.sqrt()),
        Err(e) => Value::Error(e),
    }
}

pub(crate) fn eval_mod(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some((a, b)) = two_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let (a, b) = match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Value::Error(e),
    };
    if b == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    // Excel MOD has the sign of the divisor: a - b*floor(a/b).
    Value::Number(a - b * (a / b).floor())
}

pub(crate) fn eval_power(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some((a, b)) = two_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let (a, b) = match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Value::Error(e),
    };
    let result = a.powf(b);
    if result.is_nan() {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(result)
}

/// Evaluate exactly two numeric args, or `None` if the arity is wrong.
#[allow(clippy::type_complexity)]
pub(crate) fn two_numbers(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Option<(Result<f64, ErrorValue>, Result<f64, ErrorValue>)> {
    if args.len() != 2 {
        return None;
    }
    let a = ev.eval_expr(sheet, &args[0]).as_number();
    let b = ev.eval_expr(sheet, &args[1]).as_number();
    Some((a, b))
}

pub(crate) fn eval_round(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let digits = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i32,
        Err(e) => return Value::Error(e),
    };
    let factor = 10f64.powi(digits);
    Value::Number((value * factor).round() / factor)
}

pub(crate) fn eval_concat(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let mut out = String::new();
    for arg in args {
        for value in flatten_values(ev, sheet, arg) {
            match value.as_text() {
                Ok(s) => out.push_str(&s),
                Err(e) => return Value::Error(e),
            }
        }
    }
    Value::Text(out)
}

pub(crate) fn eval_len(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_text() {
        Ok(s) => Value::Number(s.chars().count() as f64),
        Err(e) => Value::Error(e),
    }
}

/// Shared helper for `LEFT`/`RIGHT`: read `(text, count)` with `count` default 1.
pub(crate) fn text_and_count(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<(String, i64), ErrorValue> {
    if args.is_empty() || args.len() > 2 {
        return Err(ErrorValue::Value);
    }
    let text = ev.eval_expr(sheet, &args[0]).as_text()?;
    let count = match args.get(1) {
        Some(a) => ev.eval_expr(sheet, a).as_number()? as i64,
        None => 1,
    };
    if count < 0 {
        return Err(ErrorValue::Value);
    }
    Ok((text, count))
}

pub(crate) fn eval_left(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match text_and_count(ev, sheet, args) {
        Ok((text, count)) => Value::Text(text.chars().take(count as usize).collect()),
        Err(e) => Value::Error(e),
    }
}

pub(crate) fn eval_right(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match text_and_count(ev, sheet, args) {
        Ok((text, count)) => {
            let total = text.chars().count();
            let skip = total.saturating_sub(count as usize);
            Value::Text(text.chars().skip(skip).collect())
        }
        Err(e) => Value::Error(e),
    }
}

pub(crate) fn eval_mid(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let start = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let len = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    // Excel: start is 1-based and must be >= 1; length must be >= 0.
    if start < 1 || len < 0 {
        return Value::Error(ErrorValue::Value);
    }
    let skip = (start - 1) as usize;
    Value::Text(text.chars().skip(skip).take(len as usize).collect())
}

pub(crate) fn text_op(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(&str) -> String,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_text() {
        Ok(s) => Value::Text(f(&s)),
        Err(e) => Value::Error(e),
    }
}

/// `TRIM`: strip leading/trailing spaces and collapse internal runs to one.
pub(crate) fn trim_excel(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// --- Criteria-based aggregates (COUNTIF / SUMIF / AVERAGEIF) --------------

/// Flatten one argument into a flat list of values, expanding a range to every
/// cell it covers (in row-major order). A scalar argument yields one value; an
/// error encountered while evaluating a cell becomes a single `Error` value.
pub(crate) fn flatten_values(ev: &mut Evaluator<'_>, sheet: usize, arg: &Expr) -> Vec<Value> {
    if let Expr::Range(a, b) = arg {
        let Some(target) = ev.resolve_sheet(&a.sheet, sheet) else {
            return vec![Value::Error(ErrorValue::Ref)];
        };
        let (r0, c0, r1, c1) = ev.range_bounds(target, a, b);
        let area = (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64;
        if area > MAX_RANGE_CELLS {
            return vec![Value::Error(ErrorValue::Num)];
        }
        let mut out = Vec::new();
        for row in r0..=r1 {
            for col in c0..=c1 {
                out.push(ev.eval_cell(target, CellRef::new(row, col)));
            }
        }
        out
    } else {
        vec![ev.eval_expr(sheet, arg)]
    }
}

pub(crate) fn flatten_numbers(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<Vec<f64>, ErrorValue> {
    let mut out = Vec::new();
    for arg in args {
        // A structured reference names a range, so every aggregate that accepts
        // `A1:A9` must accept `Sales[Amount]` too — that is the whole point of
        // writing one. It is expanded here rather than at parse time because
        // only the evaluator can see the table's geometry.
        if let Expr::StructuredRef { table, spec } = arg {
            let Some((target, range)) = ev.resolve_structured(sheet, table.as_deref(), spec) else {
                // The table or column is gone. #REF! is what Excel shows, and
                // it is important that this is *not* silently empty: a SUM over
                // a deleted table must not read as zero.
                return Err(ErrorValue::Ref);
            };
            for row in range.start.row..=range.end.row {
                for col in range.start.col..=range.end.col {
                    match ev.eval_cell(target, CellRef::new(row, col)) {
                        Value::Number(n) => out.push(n),
                        Value::Error(e) => return Err(e),
                        // Text and logicals both skipped — see the range branch
                        // below for why they belong on the same side of this.
                        _ => {}
                    }
                }
            }
            continue;
        }
        if let Expr::Range(a, b) = arg {
            let target = ev.resolve_sheet(&a.sheet, sheet).ok_or(ErrorValue::Ref)?;
            let (r0, c0, r1, c1) = ev.range_bounds(target, a, b);
            let area = (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64;
            if area > MAX_RANGE_CELLS {
                return Err(ErrorValue::Num);
            }
            for row in r0..=r1 {
                for col in c0..=c1 {
                    match ev.eval_cell(target, CellRef::new(row, col)) {
                        Value::Number(n) => out.push(n),
                        Value::Error(e) => return Err(e),
                        // A logical **held in a cell** is ignored, exactly as
                        // text is. Excel's rule is not about the value, it is
                        // about how the value arrived: `=SUM(TRUE,1)` is 2
                        // because the logical was written as an argument, while
                        // `=SUM(A1:A2)` over `TRUE` and `1` is 1 because a
                        // reference contributes only numbers. The direct-argument
                        // branch below still coerces, which is what keeps the two
                        // halves of the rule apart.
                        //
                        // Coercing here made a column of flags corrupt every
                        // total over it, and `AVERAGE` doubly so — the boolean
                        // inflated the sum *and* the divisor. Text was already
                        // skipped by this arm, so the two were inconsistent in
                        // the same match.
                        //
                        // The `A`-suffixed functions (`AVERAGEA`, `MAXA`, …) are
                        // the ones that do count logicals in references, and they
                        // do not come through here — they have `stat_over_a`.
                        _ => {}
                    }
                }
            }
        } else {
            // Array-aware: an aggregate over a computed block has to see all of
            // it. `BYROW(a, LAMBDA(r, SUM(r)))` binds `r` to a whole row, and
            // taking the corner summed one cell and called it the row total.
            let mut take = |v: Value| -> Result<(), ErrorValue> {
                match v {
                    Value::Number(n) => out.push(n),
                    Value::Bool(b) => out.push(if b { 1.0 } else { 0.0 }),
                    Value::Empty => {}
                    Value::Text(t) => {
                        out.push(t.trim().parse::<f64>().map_err(|_| ErrorValue::Value)?)
                    }
                    Value::Error(e) => return Err(e),
                    Value::Lambda(_) => return Err(ErrorValue::Value),
                    Value::Array { .. } => unreachable!("flattened by the caller"),
                }
                Ok(())
            };
            match ev.eval_expr_array(sheet, arg) {
                Value::Array { cells, .. } => {
                    for v in cells {
                        // One level: an array cannot contain another.
                        take(v)?;
                    }
                }
                other => take(other)?,
            }
        }
    }
    Ok(out)
}

// --- Extra math -----------------------------------------------------------
