//! The built-in function library (starter subset). Aggregates flatten ranges to
//! numbers; `IF` evaluates only the taken branch.

use std::cmp::Ordering;

use casual_calc_formula::Expr;
use casual_calc_model::{CellRef, ErrorValue};

use crate::eval::Evaluator;
use crate::value::{Value, number_to_text};

/// Guard against pathological full-range aggregates (a dependency-graph with
/// range buckets is the Phase-2 optimization; this bounds the naive scan).
const MAX_RANGE_CELLS: u64 = 2_000_000;

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
    ("BETADIST", "BETADIST(x, alpha, beta, [A], [B])"),
    ("BETAINV", "BETAINV(probability, alpha, beta, [A], [B])"),
    ("BIN2DEC", "BIN2DEC(number)"),
    ("BIN2HEX", "BIN2HEX(number, [places])"),
    ("BIN2OCT", "BIN2OCT(number, [places])"),
    (
        "BINOMDIST",
        "BINOMDIST(number_s, trials, probability_s, cumulative)",
    ),
    ("BITAND", "BITAND(number1, number2)"),
    ("BITLSHIFT", "BITLSHIFT(number, shift)"),
    ("BITOR", "BITOR(number1, number2)"),
    ("BITRSHIFT", "BITRSHIFT(number, shift)"),
    ("BITXOR", "BITXOR(number1, number2)"),
    ("CEILING", "CEILING(number, significance)"),
    ("CHAR", "CHAR(number)"),
    ("CHIDIST", "CHIDIST(x, degrees_freedom)"),
    ("CHIINV", "CHIINV(probability, degrees_freedom)"),
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
    ("EXPONDIST", "EXPONDIST(x, lambda, cumulative)"),
    ("FACT", "FACT(number)"),
    ("FACTDOUBLE", "FACTDOUBLE(number)"),
    ("FALSE", "FALSE()"),
    ("FDIST", "FDIST(x, degrees_freedom1, degrees_freedom2)"),
    ("FIND", "FIND(find_text, within_text, [start])"),
    ("FINDB", "FINDB(find_text, within_text, [start_num])"),
    ("FINV", "FINV(probability, deg_freedom1, deg_freedom2)"),
    ("FISHER", "FISHER(x)"),
    ("FISHERINV", "FISHERINV(y)"),
    ("FIXED", "FIXED(number, [decimals], [no_commas])"),
    ("FLOOR", "FLOOR(number, significance)"),
    ("FORECAST", "FORECAST(x, known_y, known_x)"),
    ("FTEST", "FTEST(array1, array2)"),
    ("FV", "FV(rate, nper, pmt, [pv], [type])"),
    ("FVSCHEDULE", "FVSCHEDULE(principal, schedule)"),
    ("GAMMADIST", "GAMMADIST(x, alpha, beta, cumulative)"),
    ("GAMMAINV", "GAMMAINV(probability, alpha, beta)"),
    ("GAMMALN", "GAMMALN(x)"),
    ("GCD", "GCD(number1, …)"),
    ("GEOMEAN", "GEOMEAN(number1, …)"),
    ("GESTEP", "GESTEP(number, [step])"),
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
    ("ISOWEEKNUM", "ISOWEEKNUM(date)"),
    ("ISPMT", "ISPMT(rate, per, nper, pv)"),
    ("ISREF", "ISREF(value)"),
    ("ISTEXT", "ISTEXT(value)"),
    ("JIS", "JIS(text)"),
    ("KURT", "KURT(number1, …)"),
    ("LARGE", "LARGE(array, k)"),
    ("LCM", "LCM(number1, …)"),
    ("LEFT", "LEFT(text, [num_chars])"),
    ("LEFTB", "LEFTB(text, [num_bytes])"),
    ("LEN", "LEN(text)"),
    ("LENB", "LENB(text)"),
    ("LN", "LN(number)"),
    ("LOG", "LOG(number, [base])"),
    ("LOG10", "LOG10(number)"),
    ("LOGINV", "LOGINV(probability, mean, standard_dev)"),
    ("LOGNORMDIST", "LOGNORMDIST(x, mean, standard_dev)"),
    ("LOOKUP", "LOOKUP(value, vector, [result])"),
    ("LOWER", "LOWER(text)"),
    ("MATCH", "MATCH(lookup, array, [match_type])"),
    ("MAX", "MAX(number1, …)"),
    ("MAXA", "MAXA(value1, …)"),
    (
        "MDURATION",
        "MDURATION(settlement, maturity, coupon, yld, frequency, [basis])",
    ),
    ("MEDIAN", "MEDIAN(number1, …)"),
    ("MID", "MID(text, start_num, num_chars)"),
    ("MIDB", "MIDB(text, start_num, num_bytes)"),
    ("MIN", "MIN(number1, …)"),
    ("MINA", "MINA(value1, …)"),
    ("MINUTE", "MINUTE(serial_number)"),
    ("MIRR", "MIRR(values, finance_rate, reinvest_rate)"),
    ("MOD", "MOD(number, divisor)"),
    ("MODE", "MODE(number1, …)"),
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
    ("OFFSET", "OFFSET(reference, rows, cols, [height], [width])"),
    ("OR", "OR(logical1, …)"),
    ("PDURATION", "PDURATION(rate, pv, fv)"),
    ("PEARSON", "PEARSON(array1, array2)"),
    ("PERCENTILE", "PERCENTILE(array, k)"),
    ("PERCENTRANK", "PERCENTRANK(array, x, [significance])"),
    ("PERMUT", "PERMUT(n, k)"),
    ("PERMUTATIONA", "PERMUTATIONA(n, k)"),
    ("PI", "PI()"),
    ("PMT", "PMT(rate, nper, pv, [fv], [type])"),
    ("POISSON", "POISSON(x, mean, cumulative)"),
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
    ("QUOTIENT", "QUOTIENT(numerator, denominator)"),
    ("RADIANS", "RADIANS(angle)"),
    ("RAND", "RAND()"),
    ("RANDBETWEEN", "RANDBETWEEN(bottom, top)"),
    ("RANK", "RANK(number, ref, [order])"),
    ("RATE", "RATE(nper, pmt, pv, [fv], [type], [guess])"),
    (
        "RECEIVED",
        "RECEIVED(settlement, maturity, investment, discount, [basis])",
    ),
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
    ("SEARCH", "SEARCH(find_text, within_text, [start])"),
    ("SEARCHB", "SEARCHB(find_text, within_text, [start_num])"),
    ("SEC", "SEC(number)"),
    ("SECH", "SECH(number)"),
    ("SECOND", "SECOND(serial_number)"),
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
    ("SQRT", "SQRT(number)"),
    ("SQRTPI", "SQRTPI(number)"),
    ("STANDARDIZE", "STANDARDIZE(x, mean, standard_dev)"),
    ("STDEV", "STDEV(number1, …)"),
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
    ("TRIM", "TRIM(text)"),
    ("TRIMMEAN", "TRIMMEAN(array, percent)"),
    ("TRUE", "TRUE()"),
    ("TRUNC", "TRUNC(number, [num_digits])"),
    ("TTEST", "TTEST(array1, array2, tails, type)"),
    ("TYPE", "TYPE(value)"),
    ("UNICHAR", "UNICHAR(number)"),
    ("UNICODE", "UNICODE(text)"),
    ("UPPER", "UPPER(text)"),
    ("USDOLLAR", "USDOLLAR(number, [decimals])"),
    ("VALUE", "VALUE(text)"),
    ("VAR", "VAR(number1, …)"),
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
    ("WORKDAY", "WORKDAY(start, days, [holidays])"),
    (
        "WORKDAY.INTL",
        "WORKDAY.INTL(start, days, [weekend], [holidays])",
    ),
    ("XIRR", "XIRR(values, dates, [guess])"),
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
        "MODE" => stat_over(ev, sheet, args, mode_of),
        "SKEW" => stat_over(ev, sheet, args, skew_of),
        "KURT" => stat_over(ev, sheet, args, kurt_of),
        "VAR" => stat_over(ev, sheet, args, |ns| variance(ns, true)),
        "VARP" => stat_over(ev, sheet, args, |ns| variance(ns, false)),
        "PERCENTILE" => eval_percentile(ev, sheet, args, false),
        "QUARTILE" => eval_percentile(ev, sheet, args, true),
        "PERCENTRANK" => eval_percentrank(ev, sheet, args),
        "TRIMMEAN" => eval_trimmean(ev, sheet, args),
        "COUNTBLANK" => eval_countblank(ev, sheet, args),
        "STANDARDIZE" => eval_standardize(ev, sheet, args),
        // Paired-sample statistics: two ranges of equal length.
        "CORREL" | "PEARSON" => paired(ev, sheet, args, correlation),
        "RSQ" => paired(ev, sheet, args, |xs, ys| correlation(xs, ys).map(|r| r * r)),
        "COVAR" => paired(ev, sheet, args, |xs, ys| {
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
        "NORMSINV" => checked(ev, sheet, args, |p| {
            if p <= 0.0 || p >= 1.0 {
                Value::Error(ErrorValue::Num)
            } else {
                Value::Number(normal_quantile(p))
            }
        }),
        "NORMDIST" => eval_normdist(ev, sheet, args),
        "NORMINV" => eval_norminv(ev, sheet, args),
        "EXPONDIST" => eval_expondist(ev, sheet, args),
        "POISSON" => eval_poisson(ev, sheet, args),
        "BINOMDIST" => eval_binomdist(ev, sheet, args),
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
        "LOGINV" => eval_loginv(ev, sheet, args),
        "WEIBULL" => eval_weibull(ev, sheet, args),
        "NEGBINOMDIST" => eval_negbinomdist(ev, sheet, args),
        "HYPGEOMDIST" => eval_hypgeomdist(ev, sheet, args),
        "CRITBINOM" => eval_critbinom(ev, sheet, args),
        "CONFIDENCE" => eval_confidence(ev, sheet, args),
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
        "CHIDIST" => eval_chidist(ev, sheet, args),
        "CHIINV" => eval_chiinv(ev, sheet, args),
        "TDIST" => eval_tdist(ev, sheet, args),
        "TINV" => eval_tinv(ev, sheet, args),
        "FDIST" => eval_fdist(ev, sheet, args),
        "FINV" => eval_finv(ev, sheet, args),
        "GAMMADIST" => eval_gammadist(ev, sheet, args),
        "GAMMAINV" => eval_gammainv(ev, sheet, args),
        "BETADIST" => eval_betadist(ev, sheet, args),
        "BETAINV" => eval_betainv(ev, sheet, args),
        "ZTEST" => eval_ztest(ev, sheet, args),
        "TTEST" => eval_ttest(ev, sheet, args),
        "FTEST" => eval_ftest(ev, sheet, args),
        "CHITEST" => eval_chitest(ev, sheet, args),
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
        "RANK" => eval_rank(ev, sheet, args),
        "STDEV" => eval_stdev(ev, sheet, args, true),
        "STDEVP" => eval_stdev(ev, sheet, args, false),
        "SUMPRODUCT" => eval_sumproduct(ev, sheet, args),
        // --- Multi-criteria aggregates (M6-2) ---
        "SUMIFS" => eval_ifs_aggregate(ev, sheet, args, IfsKind::Sum),
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

fn reduce(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: fn(f64, f64) -> f64) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) if ns.is_empty() => Value::Number(0.0),
        Ok(ns) => Value::Number(ns.into_iter().reduce(f).unwrap_or(0.0)),
        Err(e) => Value::Error(e),
    }
}

fn scalar(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: fn(f64) -> f64) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => Value::Number(f(n)),
        Err(e) => Value::Error(e),
    }
}

fn eval_if(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

fn eval_iferror(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
fn eval_and_or(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], require_all: bool) -> Value {
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

fn eval_not(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, &args[0]).as_bool() {
        Ok(b) => Value::Bool(!b),
        Err(e) => Value::Error(e),
    }
}

fn eval_counta(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

fn eval_sqrt(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) if n < 0.0 => Value::Error(ErrorValue::Num),
        Ok(n) => Value::Number(n.sqrt()),
        Err(e) => Value::Error(e),
    }
}

fn eval_mod(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

fn eval_power(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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
fn two_numbers(
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

fn eval_round(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

fn eval_concat(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

fn eval_len(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_text() {
        Ok(s) => Value::Number(s.chars().count() as f64),
        Err(e) => Value::Error(e),
    }
}

/// Shared helper for `LEFT`/`RIGHT`: read `(text, count)` with `count` default 1.
fn text_and_count(
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

fn eval_left(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match text_and_count(ev, sheet, args) {
        Ok((text, count)) => Value::Text(text.chars().take(count as usize).collect()),
        Err(e) => Value::Error(e),
    }
}

fn eval_right(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match text_and_count(ev, sheet, args) {
        Ok((text, count)) => {
            let total = text.chars().count();
            let skip = total.saturating_sub(count as usize);
            Value::Text(text.chars().skip(skip).collect())
        }
        Err(e) => Value::Error(e),
    }
}

fn eval_mid(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
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

fn text_op(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: fn(&str) -> String) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_text() {
        Ok(s) => Value::Text(f(&s)),
        Err(e) => Value::Error(e),
    }
}

/// `TRIM`: strip leading/trailing spaces and collapse internal runs to one.
fn trim_excel(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// --- Criteria-based aggregates (COUNTIF / SUMIF / AVERAGEIF) --------------

fn eval_countif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let (op, operand) = parse_criteria(&ev.eval_expr(sheet, &args[1]));
    let range = flatten_values(ev, sheet, &args[0]);
    let count = range
        .iter()
        .filter(|v| !matches!(v, Value::Empty) && criterion_matches(v, op, &operand))
        .count();
    Value::Number(count as f64)
}

fn eval_sumif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match conditional_values(ev, sheet, args) {
        Ok(picked) => Value::Number(picked.iter().sum()),
        Err(e) => Value::Error(e),
    }
}

fn eval_averageif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match conditional_values(ev, sheet, args) {
        Ok(picked) if picked.is_empty() => Value::Error(ErrorValue::Div0),
        Ok(picked) => Value::Number(picked.iter().sum::<f64>() / picked.len() as f64),
        Err(e) => Value::Error(e),
    }
}

/// Shared `SUMIF`/`AVERAGEIF` core: for each cell in the criteria range that
/// matches, collect the corresponding numeric value from the sum range (or the
/// criteria range itself when no third argument is given).
fn conditional_values(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<Vec<f64>, ErrorValue> {
    if args.len() != 2 && args.len() != 3 {
        return Err(ErrorValue::Value);
    }
    let (op, operand) = parse_criteria(&ev.eval_expr(sheet, &args[1]));
    let range = flatten_values(ev, sheet, &args[0]);
    let sum_range = match args.get(2) {
        Some(a) => flatten_values(ev, sheet, a),
        None => range.clone(),
    };
    let mut out = Vec::new();
    for (i, cell) in range.iter().enumerate() {
        if matches!(cell, Value::Empty) || !criterion_matches(cell, op, &operand) {
            continue;
        }
        let Some(target) = sum_range.get(i) else {
            continue;
        };
        match target {
            Value::Number(n) => out.push(*n),
            Value::Bool(b) => out.push(if *b { 1.0 } else { 0.0 }),
            Value::Error(e) => return Err(*e),
            _ => {}
        }
    }
    Ok(out)
}

/// A comparison operator parsed from a criteria string.
#[derive(Clone, Copy)]
enum CritOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// Split a criteria value into a comparison operator and an operand string.
/// A bare value (no leading operator) means equality.
fn parse_criteria(v: &Value) -> (CritOp, String) {
    let s = v.as_text().unwrap_or_default();
    let (op, rest) = if let Some(r) = s.strip_prefix(">=") {
        (CritOp::Ge, r)
    } else if let Some(r) = s.strip_prefix("<=") {
        (CritOp::Le, r)
    } else if let Some(r) = s.strip_prefix("<>") {
        (CritOp::Ne, r)
    } else if let Some(r) = s.strip_prefix('>') {
        (CritOp::Gt, r)
    } else if let Some(r) = s.strip_prefix('<') {
        (CritOp::Lt, r)
    } else if let Some(r) = s.strip_prefix('=') {
        (CritOp::Eq, r)
    } else {
        (CritOp::Eq, s.as_str())
    };
    (op, rest.to_owned())
}

/// Does `cell` satisfy `op operand`? Numeric when both sides are numeric,
/// otherwise a case-insensitive text comparison (Excel semantics). For `=`/`<>`
/// criteria whose operand contains an unescaped `*` or `?`, Excel wildcard
/// matching is used and applies to **text** cells only.
fn criterion_matches(cell: &Value, op: CritOp, operand: &str) -> bool {
    // Wildcard text matching (Excel): `*` = any run, `?` = one char, `~` escapes
    // the next `*`/`?`/`~`. Wildcards only match text cells, not numbers/blanks.
    if matches!(op, CritOp::Eq | CritOp::Ne) && has_wildcard(operand) {
        let matched = match cell {
            Value::Text(s) => wildcard_match(operand, s),
            _ => false,
        };
        return if matches!(op, CritOp::Ne) {
            !matched
        } else {
            matched
        };
    }

    let operand_num = operand.trim().parse::<f64>().ok();
    let cell_num = match cell {
        Value::Number(n) => Some(*n),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Text(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    let ordering = match (cell_num, operand_num) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        _ => {
            let a = cell.as_text().unwrap_or_default().to_uppercase();
            // Unescape `~*`/`~?`/`~~` so a criterion can match a literal wildcard.
            let b = unescape_criteria(operand).to_uppercase();
            Some(a.cmp(&b))
        }
    };
    let Some(ordering) = ordering else {
        return false;
    };
    match op {
        CritOp::Eq => ordering == Ordering::Equal,
        CritOp::Ne => ordering != Ordering::Equal,
        CritOp::Gt => ordering == Ordering::Greater,
        CritOp::Ge => ordering != Ordering::Less,
        CritOp::Lt => ordering == Ordering::Less,
        CritOp::Le => ordering != Ordering::Greater,
    }
}

/// True if `s` contains a `*` or `?` that is not escaped by a preceding `~`.
fn has_wildcard(s: &str) -> bool {
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '~' => {
                chars.next(); // the escaped char is literal
            }
            '*' | '?' => return true,
            _ => {}
        }
    }
    false
}

/// Remove `~` escapes before `*`/`?`/`~`, leaving other characters untouched.
fn unescape_criteria(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('~', Some(&n)) if matches!(n, '*' | '?' | '~') => {
                out.push(n);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// Case-insensitive Excel wildcard match of `pattern` against `text`.
/// `*` matches any run of characters (including empty), `?` matches exactly one
/// character, and `~` escapes the following `*`/`?`/`~` to a literal.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    enum Tok {
        Any,
        One,
        Lit(char),
    }
    // Fold case up front so both pattern literals and text compare case-insensitively.
    let pat_up = pattern.to_uppercase();
    let mut toks = Vec::new();
    let mut chars = pat_up.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '~' => match chars.peek() {
                Some(&n @ ('*' | '?' | '~')) => {
                    toks.push(Tok::Lit(n));
                    chars.next();
                }
                _ => toks.push(Tok::Lit('~')),
            },
            '*' => toks.push(Tok::Any),
            '?' => toks.push(Tok::One),
            other => toks.push(Tok::Lit(other)),
        }
    }

    let text: Vec<char> = text.to_uppercase().chars().collect();
    // Classic linear-time backtracking wildcard match.
    let (mut ti, mut pi) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None; // (pattern idx after '*', text idx)
    while ti < text.len() {
        match toks.get(pi) {
            Some(Tok::One) => {
                pi += 1;
                ti += 1;
            }
            Some(Tok::Lit(c)) if *c == text[ti] => {
                pi += 1;
                ti += 1;
            }
            Some(Tok::Any) => {
                star = Some((pi + 1, ti));
                pi += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    pi = sp;
                    ti = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    while matches!(toks.get(pi), Some(Tok::Any)) {
        pi += 1;
    }
    pi == toks.len()
}

// --- Range flattening -----------------------------------------------------

/// Flatten one argument into a flat list of values, expanding a range to every
/// cell it covers (in row-major order). A scalar argument yields one value; an
/// error encountered while evaluating a cell becomes a single `Error` value.
fn flatten_values(ev: &mut Evaluator<'_>, sheet: usize, arg: &Expr) -> Vec<Value> {
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

fn flatten_numbers(
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
                        Value::Bool(b) => out.push(if b { 1.0 } else { 0.0 }),
                        Value::Error(e) => return Err(e),
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
                        Value::Bool(b) => out.push(if b { 1.0 } else { 0.0 }),
                        Value::Error(e) => return Err(e),
                        _ => {}
                    }
                }
            }
        } else {
            match ev.eval_expr(sheet, arg) {
                Value::Number(n) => out.push(n),
                Value::Bool(b) => out.push(if b { 1.0 } else { 0.0 }),
                Value::Empty => {}
                Value::Text(t) => out.push(t.trim().parse::<f64>().map_err(|_| ErrorValue::Value)?),
                Value::Error(e) => return Err(e),
            }
        }
    }
    Ok(out)
}

// --- Extra math -----------------------------------------------------------

fn eval_product(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) if ns.is_empty() => Value::Number(0.0),
        Ok(ns) => Value::Number(ns.iter().product()),
        Err(e) => Value::Error(e),
    }
}

#[derive(Clone, Copy)]
enum RoundDir {
    Up,
    Down,
}

/// `ROUNDUP`/`ROUNDDOWN`: round away from / toward zero to `digits` places.
fn eval_round_dir(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], dir: RoundDir) -> Value {
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
    let scaled = (value * factor).abs();
    let rounded = match dir {
        RoundDir::Up => scaled.ceil(),
        RoundDir::Down => scaled.floor(),
    };
    Value::Number(value.signum() * rounded / factor)
}

/// `TRUNC(number, [digits])`: truncate toward zero (digits default 0).
fn eval_trunc(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let digits = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i32,
            Err(e) => return Value::Error(e),
        },
        None => 0,
    };
    let factor = 10f64.powi(digits);
    Value::Number((value * factor).trunc() / factor)
}

/// `CEILING`/`FLOOR`: round to the nearest multiple of `significance`.
/// Excel requires number and significance to share a sign (else `#NUM!`).
fn eval_ceiling_floor(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], up: bool) -> Value {
    let Some((num, sig)) = two_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let (num, sig) = match (num, sig) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Value::Error(e),
    };
    if sig == 0.0 {
        return Value::Number(0.0);
    }
    if num != 0.0 && num.signum() != sig.signum() {
        return Value::Error(ErrorValue::Num);
    }
    let quotient = num / sig;
    let rounded = if up {
        quotient.ceil()
    } else {
        quotient.floor()
    };
    Value::Number(rounded * sig)
}

fn eval_sign(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) if n > 0.0 => Value::Number(1.0),
        Ok(n) if n < 0.0 => Value::Number(-1.0),
        Ok(_) => Value::Number(0.0),
        Err(e) => Value::Error(e),
    }
}

// --- Lookup / reference ---------------------------------------------------

/// A materialized rectangular block of cell values (row-major).
struct Grid {
    rows: usize,
    cols: usize,
    cells: Vec<Value>,
}

impl Grid {
    fn get(&self, row: usize, col: usize) -> &Value {
        &self.cells[row * self.cols + col]
    }
}

/// Evaluate one argument into a [`Grid`]; a scalar becomes a 1x1 block.
fn eval_range_2d(ev: &mut Evaluator<'_>, sheet: usize, arg: &Expr) -> Result<Grid, ErrorValue> {
    if let Expr::Range(a, b) = arg {
        let target = ev.resolve_sheet(&a.sheet, sheet).ok_or(ErrorValue::Ref)?;
        let (r0, c0, r1, c1) = ev.range_bounds(target, a, b);
        let area = (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64;
        if area > MAX_RANGE_CELLS {
            return Err(ErrorValue::Num);
        }
        let rows = (r1 - r0 + 1) as usize;
        let cols = (c1 - c0 + 1) as usize;
        let mut cells = Vec::with_capacity(rows * cols);
        for row in r0..=r1 {
            for col in c0..=c1 {
                cells.push(ev.eval_cell(target, CellRef::new(row, col)));
            }
        }
        Ok(Grid { rows, cols, cells })
    } else {
        Ok(Grid {
            rows: 1,
            cols: 1,
            cells: vec![ev.eval_expr(sheet, arg)],
        })
    }
}

/// Order two values the way lookups compare: numerically when both are numeric,
/// otherwise by case-insensitive text (matches the engine's comparison rules).
fn loose_cmp(a: &Value, b: &Value) -> Option<Ordering> {
    match (numeric_of(a), numeric_of(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y),
        _ => {
            let sa = a.as_text().unwrap_or_default().to_uppercase();
            let sb = b.as_text().unwrap_or_default().to_uppercase();
            Some(sa.cmp(&sb))
        }
    }
}

fn numeric_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => Some(*n),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// `VLOOKUP` (`vertical` true) / `HLOOKUP` (false).
fn eval_vlookup(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], vertical: bool) -> Value {
    if args.len() < 3 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let key = ev.eval_expr(sheet, &args[0]);
    if let Some(e) = key.as_error() {
        return Value::Error(e);
    }
    let grid = match eval_range_2d(ev, sheet, &args[1]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let index = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let approximate = match args.get(3) {
        Some(a) => match ev.eval_expr(sheet, a).as_bool() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        },
        None => true,
    };
    if index < 1 {
        return Value::Error(ErrorValue::Value);
    }
    let index = index as usize;
    // Length along the search axis, and bound for the return index.
    let (search_len, index_bound) = if vertical {
        (grid.rows, grid.cols)
    } else {
        (grid.cols, grid.rows)
    };
    if index > index_bound {
        return Value::Error(ErrorValue::Ref);
    }
    // Value in the search line at position `i`.
    let at = |g: &Grid, i: usize| -> Value {
        if vertical {
            g.get(i, 0).clone()
        } else {
            g.get(0, i).clone()
        }
    };
    let found = if approximate {
        // Largest entry <= key, assuming the line is sorted ascending.
        let mut best: Option<usize> = None;
        for i in 0..search_len {
            match loose_cmp(&at(&grid, i), &key) {
                Some(Ordering::Less) | Some(Ordering::Equal) => best = Some(i),
                _ => break,
            }
        }
        best
    } else {
        (0..search_len).find(|&i| loose_cmp(&at(&grid, i), &key) == Some(Ordering::Equal))
    };
    match found {
        Some(i) if vertical => grid.get(i, index - 1).clone(),
        Some(i) => grid.get(index - 1, i).clone(),
        None => Value::Error(ErrorValue::Na),
    }
}

/// `INDEX(range, row, [col])`. Row/col are 1-based; a single index selects
/// along the sole axis of a one-dimensional range.
fn eval_index(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let grid = match eval_range_2d(ev, sheet, &args[0]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let first = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let second = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => Some(n as i64),
            Err(e) => return Value::Error(e),
        },
        None => None,
    };
    let (row, col) = match second {
        Some(c) => (first, c),
        None => {
            // One index: pick the axis that has more than one line.
            if grid.rows == 1 {
                (1, first)
            } else if grid.cols == 1 {
                (first, 1)
            } else {
                return Value::Error(ErrorValue::Ref);
            }
        }
    };
    if row < 1 || col < 1 || row as usize > grid.rows || col as usize > grid.cols {
        return Value::Error(ErrorValue::Ref);
    }
    grid.get(row as usize - 1, col as usize - 1).clone()
}

/// `MATCH(lookup, range, [type])`. Type 1 (default) ascending, 0 exact,
/// -1 descending. Returns the 1-based position, or `#N/A`.
fn eval_match(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let key = ev.eval_expr(sheet, &args[0]);
    if let Some(e) = key.as_error() {
        return Value::Error(e);
    }
    let grid = match eval_range_2d(ev, sheet, &args[1]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let match_type = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    // MATCH operates on a single row or column.
    let line: Vec<&Value> = grid.cells.iter().collect();
    let found = match match_type {
        0 => line
            .iter()
            .position(|v| loose_cmp(v, &key) == Some(Ordering::Equal)),
        1 => {
            // Largest value <= key (ascending order assumed).
            let mut best = None;
            for (i, v) in line.iter().enumerate() {
                match loose_cmp(v, &key) {
                    Some(Ordering::Less) | Some(Ordering::Equal) => best = Some(i),
                    _ => break,
                }
            }
            best
        }
        _ => {
            // -1: smallest value >= key (descending order assumed).
            let mut best = None;
            for (i, v) in line.iter().enumerate() {
                match loose_cmp(v, &key) {
                    Some(Ordering::Greater) | Some(Ordering::Equal) => best = Some(i),
                    _ => break,
                }
            }
            best
        }
    };
    match found {
        Some(i) => Value::Number(i as f64 + 1.0),
        None => Value::Error(ErrorValue::Na),
    }
}

/// `CHOOSE(index, value1, value2, ...)`. Only the selected value is evaluated.
fn eval_choose(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 {
        return Value::Error(ErrorValue::Value);
    }
    let index = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let choices = &args[1..];
    if index < 1 || index as usize > choices.len() {
        return Value::Error(ErrorValue::Value);
    }
    ev.eval_expr(sheet, &choices[index as usize - 1])
}

// --- Extra text -----------------------------------------------------------

/// `SUBSTITUTE(text, old, new, [instance])`.
fn eval_substitute(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let old = match ev.eval_expr(sheet, &args[1]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let new = match ev.eval_expr(sheet, &args[2]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    if old.is_empty() {
        return Value::Text(text);
    }
    let instance = match args.get(3) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) if n < 1.0 => return Value::Error(ErrorValue::Value),
            Ok(n) => Some(n as usize),
            Err(e) => return Value::Error(e),
        },
        None => None,
    };
    match instance {
        None => Value::Text(text.replace(&old, &new)),
        Some(target) => {
            let mut out = String::with_capacity(text.len());
            let mut rest = text.as_str();
            let mut seen = 0usize;
            while let Some(pos) = rest.find(&old) {
                seen += 1;
                if seen == target {
                    out.push_str(&rest[..pos]);
                    out.push_str(&new);
                    out.push_str(&rest[pos + old.len()..]);
                    return Value::Text(out);
                }
                out.push_str(&rest[..pos + old.len()]);
                rest = &rest[pos + old.len()..];
            }
            out.push_str(rest);
            Value::Text(out)
        }
    }
}

/// `REPLACE(old_text, start, num_chars, new_text)` (1-based, over characters).
fn eval_replace(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
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
    let count = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let new = match ev.eval_expr(sheet, &args[3]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    if start < 1 || count < 0 {
        return Value::Error(ErrorValue::Value);
    }
    let chars: Vec<char> = text.chars().collect();
    let begin = (start as usize - 1).min(chars.len());
    let end = (begin + count as usize).min(chars.len());
    let mut out: String = chars[..begin].iter().collect();
    out.push_str(&new);
    out.extend(chars[end..].iter());
    Value::Text(out)
}

/// `FIND` (case-sensitive) / `SEARCH` (case-insensitive). 1-based; `#VALUE!`
/// when not found or `start` is out of range.
fn eval_find_search(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    case_sensitive: bool,
) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let needle = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let haystack = match ev.eval_expr(sheet, &args[1]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let start = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    let hay_chars: Vec<char> = haystack.chars().collect();
    if start < 1 || start as usize > hay_chars.len() + 1 {
        return Value::Error(ErrorValue::Value);
    }
    let skip = start as usize - 1;
    let (needle, tail): (String, String) = if case_sensitive {
        (needle, hay_chars[skip..].iter().collect())
    } else {
        (
            needle.to_uppercase(),
            hay_chars[skip..].iter().collect::<String>().to_uppercase(),
        )
    };
    match tail.find(&needle) {
        Some(byte_pos) => {
            // Convert the byte offset within `tail` to a character offset.
            let char_off = tail[..byte_pos].chars().count();
            Value::Number((skip + char_off + 1) as f64)
        }
        None => Value::Error(ErrorValue::Value),
    }
}

/// `VALUE(text)`: parse text as a number.
fn eval_value(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let text = match ev.eval_expr(sheet, arg).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    match text.trim().parse::<f64>() {
        Ok(n) => Value::Number(n),
        Err(_) => Value::Error(ErrorValue::Value),
    }
}

/// `PROPER`: capitalize the first letter of each word, lowercase the rest.
fn proper_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at_word_start = true;
    for ch in s.chars() {
        if ch.is_alphabetic() {
            if at_word_start {
                out.extend(ch.to_uppercase());
            } else {
                out.extend(ch.to_lowercase());
            }
            at_word_start = false;
        } else {
            out.push(ch);
            at_word_start = true;
        }
    }
    out
}

/// `REPT(text, count)`.
fn eval_rept(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let count = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    if count < 0 {
        return Value::Error(ErrorValue::Value);
    }
    Value::Text(text.repeat(count as usize))
}

/// `EXACT(a, b)`: case-sensitive text equality.
fn eval_exact(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let a = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let b = match ev.eval_expr(sheet, &args[1]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    Value::Bool(a == b)
}

// --- Dates (deterministic, 1900 serial system) ----------------------------

/// Days from the civil date `(y, m, d)` to 1970-01-01 (Howard Hinnant's
/// algorithm). Proleptic Gregorian; the inverse of [`serial_to_ymd`].
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Convert a civil date to an Excel (1900-system) serial day number.
fn ymd_to_serial(y: i64, m: i64, d: i64) -> i64 {
    days_from_civil(y, m, d) + 25_569
}

/// Convert an Excel serial day number to `(year, month, day)`.
fn serial_to_ymd(serial_days: i64) -> (i64, i64, i64) {
    let mut z = serial_days - 25_569 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    z -= era * 146_097;
    let doe = z;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn days_in_month(y: i64, m: i64) -> i64 {
    // Normalize so month 13 rolls to January of the next year, etc.
    let ny = y + (m - 1).div_euclid(12);
    let nm = (m - 1).rem_euclid(12) + 1;
    let next = if nm == 12 {
        ymd_to_serial(ny + 1, 1, 1)
    } else {
        ymd_to_serial(ny, nm + 1, 1)
    };
    next - ymd_to_serial(ny, nm, 1)
}

/// `DATE(year, month, day)`. Month/day overflow rolls into adjacent months
/// (Excel semantics); years 0-1899 are offset by 1900.
fn eval_date(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let mut year = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let month = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    let day = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    if (0..1900).contains(&year) {
        year += 1900;
    }
    // Normalize the month into 1..=12, carrying into the year, then add the
    // day offset (which itself may push across month boundaries).
    let ny = year + (month - 1).div_euclid(12);
    let nm = (month - 1).rem_euclid(12) + 1;
    let serial = ymd_to_serial(ny, nm, 1) + (day - 1);
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(serial as f64)
}

#[derive(Clone, Copy)]
enum DatePart {
    Year,
    Month,
    Day,
}

fn eval_date_part(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], part: DatePart) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let serial = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    let (y, m, d) = serial_to_ymd(serial);
    let out = match part {
        DatePart::Year => y,
        DatePart::Month => m,
        DatePart::Day => d,
    };
    Value::Number(out as f64)
}

/// `WEEKDAY(serial, [type])`. Type 1 (default) Sun=1..Sat=7, type 2
/// Mon=1..Sun=7, type 3 Mon=0..Sun=6.
fn eval_weekday(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let serial = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let kind = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    // Days since the Unix epoch; 1970-01-01 was a Thursday.
    let unix = serial - 25_569;
    let dow_sun0 = (unix + 4).rem_euclid(7); // 0 = Sunday .. 6 = Saturday
    let out = match kind {
        1 => dow_sun0 + 1,
        2 => (dow_sun0 + 6).rem_euclid(7) + 1,
        3 => (dow_sun0 + 6).rem_euclid(7),
        _ => return Value::Error(ErrorValue::Num),
    };
    Value::Number(out as f64)
}

/// `EDATE` (`eomonth` false) advances by whole months keeping the day (clamped
/// to the month length). `EOMONTH` (true) returns the last day of that month.
fn eval_edate(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], eomonth: bool) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let serial = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let months = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n as i64,
        Err(e) => return Value::Error(e),
    };
    if serial < 0 {
        return Value::Error(ErrorValue::Num);
    }
    let (y, m, d) = serial_to_ymd(serial);
    let total = m - 1 + months;
    let ny = y + total.div_euclid(12);
    let nm = total.rem_euclid(12) + 1;
    let last = days_in_month(ny, nm);
    let day = if eomonth { last } else { d.min(last) };
    let out = ymd_to_serial(ny, nm, day);
    if out < 0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(out as f64)
}

// --- M6-2 built-ins --------------------------------------------------------

/// The IS-family: evaluate the single argument and test the resulting value.
fn is_predicate(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    test: fn(&Value) -> bool,
) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    Value::Bool(test(&ev.eval_expr(sheet, arg)))
}

/// ISEVEN / ISODD: truncate toward zero, then test parity.
fn eval_parity(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], even: bool) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => Value::Bool(((n.trunc() as i64).rem_euclid(2) == 0) == even),
        Err(e) => Value::Error(e),
    }
}

/// IFS(test1, value1, test2, value2, …): first TRUE test's value, else #N/A.
fn eval_ifs(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || !args.len().is_multiple_of(2) {
        return Value::Error(ErrorValue::Value);
    }
    for pair in args.chunks(2) {
        match ev.eval_expr(sheet, &pair[0]).as_bool() {
            Ok(true) => return ev.eval_expr(sheet, &pair[1]),
            Ok(false) => {}
            Err(e) => return Value::Error(e),
        }
    }
    Value::Error(ErrorValue::Na)
}

/// SWITCH(expr, v1, r1, …, [default]).
fn eval_switch(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 {
        return Value::Error(ErrorValue::Value);
    }
    let subject = ev.eval_expr(sheet, &args[0]);
    let rest = &args[1..];
    let mut i = 0;
    while i + 1 < rest.len() {
        let candidate = ev.eval_expr(sheet, &rest[i]);
        if values_equal(&subject, &candidate) {
            return ev.eval_expr(sheet, &rest[i + 1]);
        }
        i += 2;
    }
    // A trailing odd argument is the default.
    if rest.len() % 2 == 1 {
        return ev.eval_expr(sheet, &rest[rest.len() - 1]);
    }
    Value::Error(ErrorValue::Na)
}

/// Excel equality for SWITCH: numeric when both numeric, else case-insensitive text.
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x == y,
        _ => a
            .as_text()
            .unwrap_or_default()
            .eq_ignore_ascii_case(&b.as_text().unwrap_or_default()),
    }
}

/// IFNA(value, value_if_na): substitute only on #N/A.
fn eval_ifna(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, &args[0]) {
        Value::Error(ErrorValue::Na) => ev.eval_expr(sheet, &args[1]),
        v => v,
    }
}

/// MEDIAN over all numeric arguments.
fn eval_median(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(mut ns) if !ns.is_empty() => {
            ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            let m = ns.len() / 2;
            let med = if ns.len() % 2 == 1 {
                ns[m]
            } else {
                (ns[m - 1] + ns[m]) / 2.0
            };
            Value::Number(med)
        }
        Ok(_) => Value::Error(ErrorValue::Num),
        Err(e) => Value::Error(e),
    }
}

/// LARGE(array, k) / SMALL(array, k): k-th largest/smallest (1-based).
fn eval_large_small(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], large: bool) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let k = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let mut ns = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    if k < 1 || k as usize > ns.len() {
        return Value::Error(ErrorValue::Num);
    }
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let idx = if large {
        ns.len() - k as usize
    } else {
        k as usize - 1
    };
    Value::Number(ns[idx])
}

/// RANK(number, ref, [order]): position of `number` within `ref` (1-based).
/// `order` 0/omitted = descending, non-zero = ascending. Ties share a rank.
fn eval_rank(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let target = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let ns = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let ascending = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n != 0.0,
            Err(e) => return Value::Error(e),
        },
        None => false,
    };
    if !ns.contains(&target) {
        return Value::Error(ErrorValue::Na);
    }
    let rank = if ascending {
        1 + ns.iter().filter(|&&n| n < target).count()
    } else {
        1 + ns.iter().filter(|&&n| n > target).count()
    };
    Value::Number(rank as f64)
}

/// STDEV (sample, n-1) / STDEVP (population, n).
fn eval_stdev(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], sample: bool) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) => {
            let n = ns.len();
            if n < if sample { 2 } else { 1 } {
                return Value::Error(ErrorValue::Div0);
            }
            let mean = ns.iter().sum::<f64>() / n as f64;
            let ss: f64 = ns.iter().map(|x| (x - mean).powi(2)).sum();
            let denom = if sample { (n - 1) as f64 } else { n as f64 };
            Value::Number((ss / denom).sqrt())
        }
        Err(e) => Value::Error(e),
    }
}

/// SUMPRODUCT: element-wise product of equal-length arrays, then summed.
fn eval_sumproduct(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() {
        return Value::Error(ErrorValue::Value);
    }
    let mut cols: Vec<Vec<f64>> = Vec::new();
    for arg in args {
        let mut nums = Vec::new();
        for v in flatten_values(ev, sheet, arg) {
            match v {
                Value::Number(n) => nums.push(n),
                Value::Bool(b) => nums.push(if b { 1.0 } else { 0.0 }),
                Value::Error(e) => return Value::Error(e),
                _ => nums.push(0.0), // text/empty contribute 0, per Excel
            }
        }
        cols.push(nums);
    }
    let len = cols[0].len();
    if cols.iter().any(|c| c.len() != len) {
        return Value::Error(ErrorValue::Value);
    }
    let mut total = 0.0;
    for i in 0..len {
        total += cols.iter().map(|c| c[i]).product::<f64>();
    }
    Value::Number(total)
}

/// ROWS / COLUMNS: the row/column count of a range (a lone cell ref is 1×1).
fn eval_dim(_ev: &mut Evaluator<'_>, _sheet: usize, args: &[Expr], rows: bool) -> Value {
    match args.first() {
        Some(Expr::Range(a, b)) => {
            let n = if rows {
                a.row.max(b.row) - a.row.min(b.row) + 1
            } else {
                a.col.max(b.col) - a.col.min(b.col) + 1
            };
            Value::Number(n as f64)
        }
        Some(Expr::Reference(_)) => Value::Number(1.0),
        _ => Value::Error(ErrorValue::Value),
    }
}

/// TEXTJOIN(delimiter, ignore_empty, text1, …).
/// TEXT(value, format_code): format a number with a SpreadsheetML format code,
/// via the same engine the grid uses to display cells (so they never drift).
/// A non-numeric first argument is returned as its text unchanged (Excel's
/// behavior when the value is already text).
fn eval_text(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let code = match ev.eval_expr(sheet, &args[1]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    match ev.eval_expr(sheet, &args[0]) {
        Value::Error(e) => Value::Error(e),
        Value::Number(n) => Value::Text(casual_calc_layout::format_number(n, &code)),
        Value::Bool(b) => Value::Text(if b { "TRUE" } else { "FALSE" }.to_owned()),
        // Text (or empty) already prints as itself.
        v => Value::Text(v.as_text().unwrap_or_default()),
    }
}

fn eval_textjoin(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 {
        return Value::Error(ErrorValue::Value);
    }
    let delim = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(s) => s,
        Err(e) => return Value::Error(e),
    };
    let ignore_empty = match ev.eval_expr(sheet, &args[1]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    let mut parts = Vec::new();
    for arg in &args[2..] {
        for v in flatten_values(ev, sheet, arg) {
            if let Value::Error(e) = v {
                return Value::Error(e);
            }
            let s = v.as_text().unwrap_or_default();
            if ignore_empty && s.is_empty() {
                continue;
            }
            parts.push(s);
        }
    }
    Value::Text(parts.join(&delim))
}

/// Which aggregate a `*IFS` call computes over the matched positions.
enum IfsKind {
    Sum,
    Average,
}

/// SUMIFS / AVERAGEIFS: an aggregate range followed by (range, criteria) pairs.
fn eval_ifs_aggregate(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], kind: IfsKind) -> Value {
    if args.len() < 3 || args.len().is_multiple_of(2) {
        return Value::Error(ErrorValue::Value);
    }
    let agg = flatten_values(ev, sheet, &args[0]);
    let keep = match ifs_matches(ev, sheet, &args[1..], agg.len()) {
        Ok(m) => m,
        Err(e) => return Value::Error(e),
    };
    let mut picked = Vec::new();
    for (i, &k) in keep.iter().enumerate() {
        if !k {
            continue;
        }
        match agg.get(i) {
            Some(Value::Number(n)) => picked.push(*n),
            Some(Value::Bool(b)) => picked.push(if *b { 1.0 } else { 0.0 }),
            Some(Value::Error(e)) => return Value::Error(*e),
            _ => {}
        }
    }
    match kind {
        IfsKind::Sum => Value::Number(picked.iter().sum()),
        IfsKind::Average if picked.is_empty() => Value::Error(ErrorValue::Div0),
        IfsKind::Average => Value::Number(picked.iter().sum::<f64>() / picked.len() as f64),
    }
}

/// COUNTIFS: count positions satisfying every (range, criteria) pair.
fn eval_countifs(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || !args.len().is_multiple_of(2) {
        return Value::Error(ErrorValue::Value);
    }
    let first = flatten_values(ev, sheet, &args[0]);
    match ifs_matches(ev, sheet, args, first.len()) {
        Ok(m) => Value::Number(m.iter().filter(|&&k| k).count() as f64),
        Err(e) => Value::Error(e),
    }
}

/// Fold consecutive (range, criteria) pairs into a per-position keep mask of
/// length `len` (logical AND across pairs). Ranges must all match `len`.
fn ifs_matches(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    pairs: &[Expr],
    len: usize,
) -> Result<Vec<bool>, ErrorValue> {
    let mut keep = vec![true; len];
    let mut i = 0;
    while i + 1 < pairs.len() {
        let (op, operand) = parse_criteria(&ev.eval_expr(sheet, &pairs[i + 1]));
        let range = flatten_values(ev, sheet, &pairs[i]);
        if range.len() != len {
            return Err(ErrorValue::Value);
        }
        for (j, cell) in range.iter().enumerate() {
            if matches!(cell, Value::Empty) || !criterion_matches(cell, op, &operand) {
                keep[j] = false;
            }
        }
        i += 2;
    }
    Ok(keep)
}

/// ROW / COLUMN: the 1-based row/column of a reference (top-left of a range),
/// or of the calling cell when no argument is given.
fn eval_row_col(ev: &mut Evaluator<'_>, args: &[Expr], row: bool) -> Value {
    let index = match args.first() {
        None => match ev.current_cell() {
            Some((_, at)) => {
                if row {
                    at.row
                } else {
                    at.col
                }
            }
            None => return Value::Error(ErrorValue::Value),
        },
        Some(Expr::Reference(r)) => {
            if row {
                r.row
            } else {
                r.col
            }
        }
        Some(Expr::Range(a, _)) => {
            if row {
                a.row
            } else {
                a.col
            }
        }
        Some(_) => return Value::Error(ErrorValue::Value),
    };
    Value::Number((index + 1) as f64)
}

// --- Maths helpers ---------------------------------------------------------

/// A unary function that can fail. Excel answers with an error value where IEEE
/// arithmetic would produce NaN or an infinity, so the closure returns a
/// [`Value`] rather than an `f64`.
fn checked(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: fn(f64) -> Value) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => f(n),
        Err(e) => Value::Error(e),
    }
}

/// A result outside the function's domain is `#NUM!`, which is what Excel
/// reports for `ASIN(2)` or `LN(-1)` where the maths yields NaN.
fn domain(v: f64) -> Value {
    if v.is_finite() {
        Value::Number(v)
    } else {
        Value::Error(ErrorValue::Num)
    }
}

/// A non-finite result becomes `err`; used where a zero denominator means the
/// answer is a division error rather than an infinity.
fn finite_or(v: f64, err: ErrorValue) -> Value {
    if v.is_finite() {
        Value::Number(v)
    } else {
        Value::Error(err)
    }
}

/// Round away from zero to the next multiple of `step`, preserving sign. Zero
/// stays zero: `EVEN(0)` is 0, not 2.
fn round_away_to(n: f64, step: f64) -> f64 {
    if n == 0.0 {
        return 0.0;
    }
    let scaled = (n.abs() / step).ceil() * step;
    if n < 0.0 { -scaled } else { scaled }
}

/// `ODD` rounds away from zero to the next odd integer; `ODD(0)` is 1.
fn eval_odd(n: f64) -> f64 {
    if n == 0.0 {
        return 1.0;
    }
    let up = ((n.abs() + 1.0) / 2.0).ceil() * 2.0 - 1.0;
    if n < 0.0 { -up } else { up }
}

/// `n!` for a non-negative integer. Negative input is `#NUM!`; anything past
/// 170 overflows an `f64`, which Excel also reports as `#NUM!` rather than
/// returning an infinity.
fn factorial(n: f64) -> Value {
    if !(0.0..=170.0).contains(&n) {
        return Value::Error(ErrorValue::Num);
    }
    let mut acc = 1.0f64;
    for i in 2..=(n as u64) {
        acc *= i as f64;
    }
    Value::Number(acc)
}

/// The double factorial `n!!` — every other term down to 1 or 2.
fn factorial_double(n: f64) -> Value {
    let n = n.trunc();
    if n < -1.0 {
        return Value::Error(ErrorValue::Num);
    }
    let mut acc = 1.0f64;
    let mut i = n;
    while i > 1.0 {
        acc *= i;
        i -= 2.0;
        if !acc.is_finite() {
            return Value::Error(ErrorValue::Num);
        }
    }
    Value::Number(acc)
}

/// `ATAN2(x, y)`. OOXML orders the arguments x-then-y, the reverse of the
/// `atan2(y, x)` every maths library uses; swapping them here is the whole
/// point of the function existing separately.
fn eval_atan2(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [x, y] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if x == 0.0 && y == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    Value::Number(y.atan2(x))
}

/// `LOG(number, [base])`, base 10 when omitted.
fn eval_log(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let n = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let base = if args.len() == 2 {
        match ev.eval_expr(sheet, &args[1]).as_number() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        }
    } else {
        10.0
    };
    if n <= 0.0 || base <= 0.0 || base == 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    domain(n.log(base))
}

/// `QUOTIENT` — the integer part of a division, discarding the remainder.
fn eval_quotient(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match pair_of_numbers(ev, sheet, args) {
        Ok([_, 0.0]) => Value::Error(ErrorValue::Div0),
        Ok([a, b]) => Value::Number((a / b).trunc()),
        Err(e) => e,
    }
}

/// `MROUND` — round to the nearest multiple. Excel requires the number and the
/// multiple to share a sign, and reports `#NUM!` when they do not.
fn eval_mround(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    match pair_of_numbers(ev, sheet, args) {
        Ok([n, m]) => {
            if m == 0.0 {
                return Value::Number(0.0);
            }
            if n.signum() != m.signum() && n != 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number((n / m).round() * m)
        }
        Err(e) => e,
    }
}

/// `COMBIN(n, k)`, or `COMBINA` for combinations with repetition.
fn eval_combin(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], repeat: bool) -> Value {
    match pair_of_numbers(ev, sheet, args) {
        Ok([n, k]) => {
            let (n, k) = (n.trunc(), k.trunc());
            if n < 0.0 || k < 0.0 || (!repeat && k > n) {
                return Value::Error(ErrorValue::Num);
            }
            // COMBINA(n, k) = COMBIN(n + k - 1, k).
            let (n, k) = if repeat { (n + k - 1.0, k) } else { (n, k) };
            binomial(n, k)
        }
        Err(e) => e,
    }
}

/// `n choose k`, accumulated term by term so the intermediate products stay
/// representable — computing `n!/(k!(n-k)!)` directly overflows well before the
/// result does.
fn binomial(n: f64, k: f64) -> Value {
    if k > n {
        return Value::Number(0.0);
    }
    let k = k.min(n - k);
    let mut acc = 1.0f64;
    let mut i = 0.0;
    while i < k {
        acc = acc * (n - i) / (i + 1.0);
        i += 1.0;
    }
    if acc.is_finite() {
        Value::Number(acc.round())
    } else {
        Value::Error(ErrorValue::Num)
    }
}

/// `PERMUT(n, k)`, or `PERMUTATIONA` for permutations with repetition.
fn eval_permut(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], repeat: bool) -> Value {
    match pair_of_numbers(ev, sheet, args) {
        Ok([n, k]) => {
            let (n, k) = (n.trunc(), k.trunc());
            if n < 0.0 || k < 0.0 || (!repeat && k > n) {
                return Value::Error(ErrorValue::Num);
            }
            if repeat {
                return finite_or(n.powf(k), ErrorValue::Num);
            }
            let mut acc = 1.0f64;
            let mut i = 0.0;
            while i < k {
                acc *= n - i;
                i += 1.0;
            }
            finite_or(acc, ErrorValue::Num)
        }
        Err(e) => e,
    }
}

/// `GCD` / `LCM` over every number in the arguments. Both are defined on
/// non-negative integers, and the fractional part is truncated as Excel does.
fn eval_gcd_lcm(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], gcd_mode: bool) -> Value {
    let numbers = match flatten_numbers(ev, sheet, args) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    if numbers.is_empty() {
        return Value::Error(ErrorValue::Value);
    }
    let mut acc: u64 = if gcd_mode { 0 } else { 1 };
    for n in numbers {
        if n < 0.0 {
            return Value::Error(ErrorValue::Num);
        }
        let v = n.trunc() as u64;
        acc = if gcd_mode {
            gcd(acc, v)
        } else if v == 0 {
            return Value::Number(0.0);
        } else {
            match (acc / gcd(acc, v)).checked_mul(v) {
                Some(l) => l,
                None => return Value::Error(ErrorValue::Num),
            }
        };
    }
    Value::Number(acc as f64)
}

fn gcd(a: u64, b: u64) -> u64 {
    let (mut a, mut b) = (a, b);
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// `MULTINOMIAL` — `(Σx)! / Πx!`, built up term by term to avoid overflowing on
/// the factorials when the result itself is small.
fn eval_multinomial(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let numbers = match flatten_numbers(ev, sheet, args) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let mut acc = 1.0f64;
    let mut running = 0.0f64;
    for n in numbers {
        if n < 0.0 {
            return Value::Error(ErrorValue::Num);
        }
        let n = n.trunc();
        running += n;
        match binomial(running, n) {
            Value::Number(c) => acc *= c,
            other => return other,
        }
    }
    finite_or(acc, ErrorValue::Num)
}

/// `SERIESSUM(x, n, m, coefficients)` — the power series
/// `Σ coefficient_i · x^(n + i·m)`.
fn eval_seriessum(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let mut scalars = [0.0f64; 3];
    for (i, slot) in scalars.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(v) => *slot = v,
            Err(e) => return Value::Error(e),
        }
    }
    let [x, n, m] = scalars;
    let coefficients = match flatten_numbers(ev, sheet, &args[3..]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let mut total = 0.0f64;
    for (i, c) in coefficients.iter().enumerate() {
        total += c * x.powf(n + (i as f64) * m);
    }
    finite_or(total, ErrorValue::Num)
}

/// Evaluate exactly two arguments as numbers, or the error to report instead.
fn pair_of_numbers(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Result<[f64; 2], Value> {
    if args.len() != 2 {
        return Err(Value::Error(ErrorValue::Value));
    }
    let a = ev
        .eval_expr(sheet, &args[0])
        .as_number()
        .map_err(Value::Error)?;
    let b = ev
        .eval_expr(sheet, &args[1])
        .as_number()
        .map_err(Value::Error)?;
    Ok([a, b])
}

// --- Logical and information helpers ---------------------------------------

/// A function taking no arguments at all.
fn nullary(args: &[Expr], value: Value) -> Value {
    if args.is_empty() {
        value
    } else {
        Value::Error(ErrorValue::Value)
    }
}

/// `N(value)` — the numeric reading of a value.
///
/// Not the same as a coercion: text is `0` rather than an error, `TRUE` is 1,
/// and an error propagates. That asymmetry is the function's entire purpose, so
/// routing it through the ordinary `as_number` would defeat it.
fn eval_n(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    match ev.eval_expr(sheet, arg) {
        Value::Number(n) => Value::Number(n),
        Value::Bool(b) => Value::Number(if b { 1.0 } else { 0.0 }),
        Value::Error(e) => Value::Error(e),
        Value::Text(_) | Value::Empty => Value::Number(0.0),
    }
}

/// `TYPE(value)` — 1 number, 2 text, 4 logical, 16 error, 64 array.
fn eval_type(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let code = match ev.eval_expr(sheet, arg) {
        // An empty cell reads as a number here, matching Excel: TYPE of a blank
        // is 1, not a distinct "empty" code.
        Value::Number(_) | Value::Empty => 1.0,
        Value::Text(_) => 2.0,
        Value::Bool(_) => 4.0,
        Value::Error(_) => 16.0,
    };
    Value::Number(code)
}

/// `ERROR.TYPE(error)` — the ordinal of an error value, or `#N/A` when the
/// argument is not an error at all.
fn eval_error_type(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg) {
        Value::Error(e) => Value::Number(match e {
            ErrorValue::Null => 1.0,
            ErrorValue::Div0 => 2.0,
            ErrorValue::Value => 3.0,
            ErrorValue::Ref => 4.0,
            ErrorValue::Name => 5.0,
            ErrorValue::Num => 6.0,
            ErrorValue::Na => 7.0,
            // #SPILL! post-dates the 5th edition, which stops at 7. Excel
            // numbers it 9 (8 being #GETTING_DATA), so that is what a workbook
            // round-tripped through Excel expects to see.
            ErrorValue::Spill => 9.0,
        }),
        // Not an error: the answer is itself #N/A, not a number.
        _ => Value::Error(ErrorValue::Na),
    }
}

/// The zero-based index of a sheet by name, case-insensitively as Excel
/// compares sheet names.
fn sheet_index_by_name(ev: &Evaluator<'_>, name: &str) -> Option<usize> {
    ev.workbook()
        .sheets
        .iter()
        .position(|s| s.name.eq_ignore_ascii_case(name))
}

/// `ISREF(value)` — whether the argument *is* a reference.
///
/// Decided from the expression rather than its value, because by the time a
/// function receives an argument the evaluator has already resolved a reference
/// to its contents; asking the value would answer "no" for every reference.
fn eval_is_ref(args: &[Expr]) -> Value {
    match args {
        [expr] => Value::Bool(matches!(expr, Expr::Reference(_) | Expr::Range(_, _))),
        _ => Value::Error(ErrorValue::Value),
    }
}

/// `ISFORMULA(reference)` — whether the referenced cell holds a formula.
fn eval_is_formula(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [Expr::Reference(reference)] = args else {
        // Anything that is not a plain reference cannot hold a formula, and
        // Excel answers #VALUE! rather than FALSE — the difference matters,
        // since FALSE would read as "that cell has no formula".
        return Value::Error(ErrorValue::Value);
    };
    let at = CellRef::new(reference.row, reference.col);
    let has = ev
        .workbook()
        .sheets
        .get(sheet)
        .and_then(|s| s.cells.get(at))
        .is_some_and(|c| c.formula.is_some());
    Value::Bool(has)
}

// --- Date and time helpers -------------------------------------------------

/// `TIME(h, m, s)` — a fraction of a day.
///
/// The components roll over rather than erroring: `TIME(25,0,0)` is 1:00, which
/// is what makes the function usable for arithmetic.
fn eval_time(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let mut parts = [0.0f64; 3];
    for (i, slot) in parts.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(v) => *slot = v.trunc(),
            Err(e) => return Value::Error(e),
        }
    }
    let seconds = parts[0] * 3600.0 + parts[1] * 60.0 + parts[2];
    if seconds < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((seconds % 86_400.0) / 86_400.0)
}

/// `HOUR`/`MINUTE`/`SECOND` — the component of a serial's time-of-day.
fn eval_time_part(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], unit: f64) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let serial = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    if serial < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    // Round to the nearest second before splitting: a time stored as a binary
    // fraction is very often a hair under, so truncating raw gives 59 minutes
    // where the sheet plainly shows 60.
    let seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    // `seconds` is within one day, so the hour needs no wrap; minutes and
    // seconds take the remainder within the next larger unit.
    let value = match unit as i64 {
        3600 => seconds / 3600,
        60 => (seconds / 60) % 60,
        _ => seconds % 60,
    };
    Value::Number(value as f64)
}

/// `DAYS360(start, end, [european])` — the 360-day year used in bond maths.
fn eval_days360(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, end) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a, b),
        Err(e) => return e,
    };
    let european = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_bool() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        },
        None => false,
    };
    let (y1, m1, mut d1) = serial_to_ymd(start.trunc() as i64);
    let (y2, m2, mut d2) = serial_to_ymd(end.trunc() as i64);
    if european {
        d1 = d1.min(30);
        d2 = d2.min(30);
    } else {
        // The US convention: only after clamping the start does a 31st end date
        // move, which is why these two cannot be written symmetrically.
        if d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 && d1 == 30 {
            d2 = 30;
        }
    }
    Value::Number(((y2 - y1) * 360 + (m2 - m1) * 30 + (d2 - d1)) as f64)
}

/// `DATEDIF(start, end, unit)` — whole years, months or days between dates.
fn eval_datedif(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, end) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc() as i64, b.trunc() as i64),
        Err(e) => return e,
    };
    let unit = match ev.eval_expr(sheet, &args[2]) {
        Value::Text(t) => t.to_ascii_uppercase(),
        Value::Error(e) => return Value::Error(e),
        _ => return Value::Error(ErrorValue::Value),
    };
    if end < start {
        // Excel reports #NUM! rather than a negative span.
        return Value::Error(ErrorValue::Num);
    }
    let (y1, m1, d1) = serial_to_ymd(start);
    let (y2, m2, d2) = serial_to_ymd(end);
    let mut months = (y2 - y1) * 12 + (m2 - m1);
    if d2 < d1 {
        months -= 1;
    }
    Value::Number(match unit.as_str() {
        "D" => (end - start) as f64,
        "M" => months as f64,
        "Y" => (months / 12) as f64,
        // Months ignoring years, days ignoring months, days ignoring years.
        "YM" => (months % 12) as f64,
        "MD" => {
            let anchor = ymd_to_serial(y2, m2 - i64::from(d2 < d1), d1);
            (end - anchor) as f64
        }
        "YD" => {
            let anchor = ymd_to_serial(y2 - i64::from((m2, d2) < (m1, d1)), m1, d1);
            (end - anchor) as f64
        }
        _ => return Value::Error(ErrorValue::Num),
    })
}

/// The weekday of a serial, 0 = Sunday.
fn weekday_of(serial: i64) -> i64 {
    // Serial 1 is 1900-01-01, a Monday under Excel's calendar.
    (serial + 6).rem_euclid(7)
}

/// `WEEKNUM(serial, [type])` — the week of the year, counting from the week
/// containing 1 January.
fn eval_weeknum(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let serial = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let start_day = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 1,
    };
    // Types 1 and 17 start on Sunday, 2 and 11 on Monday, 12..=17 on the day
    // (type - 10). ISO week numbering is type 21 and is ISOWEEKNUM's job.
    let first_weekday = match start_day {
        1 | 17 => 0,
        2 | 11 => 1,
        12..=16 => (start_day - 10) % 7,
        21 => return eval_isoweeknum(ev, sheet, &args[..1]),
        _ => return Value::Error(ErrorValue::Num),
    };
    let (year, _, _) = serial_to_ymd(serial);
    let jan1 = ymd_to_serial(year, 1, 1);
    let offset = (weekday_of(jan1) - first_weekday).rem_euclid(7);
    Value::Number(((serial - jan1 + offset) / 7 + 1) as f64)
}

/// `ISOWEEKNUM` — ISO 8601 weeks, which start on Monday and belong to the year
/// containing their Thursday.
fn eval_isoweeknum(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let serial = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    // Shift to the Thursday of this week; its year is the ISO week-year, which
    // is what makes 1 January sometimes belong to week 52 of the year before.
    let iso_weekday = (weekday_of(serial) + 6).rem_euclid(7); // 0 = Monday
    let thursday = serial - iso_weekday + 3;
    let (year, _, _) = serial_to_ymd(thursday);
    let jan1 = ymd_to_serial(year, 1, 1);
    Value::Number(((thursday - jan1) / 7 + 1) as f64)
}

/// `YEARFRAC(start, end, [basis])` — the fraction of a year between two dates.
fn eval_yearfrac(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, end) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc() as i64, b.trunc() as i64),
        Err(e) => return e,
    };
    let basis = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n as i64,
            Err(e) => return Value::Error(e),
        },
        None => 0,
    };
    let (lo, hi) = (start.min(end), start.max(end));
    let days = (hi - lo) as f64;
    let frac = match basis {
        // The day-count conventions. Getting one wrong gives an answer that is
        // close enough to look right and wrong enough to matter in interest.
        0 => {
            let d = match eval_days360_serials(lo, hi, false) {
                Some(d) => d,
                None => return Value::Error(ErrorValue::Num),
            };
            d as f64 / 360.0
        }
        1 => days / average_year_length(lo, hi),
        2 => days / 360.0,
        3 => days / 365.0,
        4 => {
            let d = match eval_days360_serials(lo, hi, true) {
                Some(d) => d,
                None => return Value::Error(ErrorValue::Num),
            };
            d as f64 / 360.0
        }
        _ => return Value::Error(ErrorValue::Num),
    };
    Value::Number(frac)
}

/// The 360-day span between two serials, shared by DAYS360 and YEARFRAC.
fn eval_days360_serials(start: i64, end: i64, european: bool) -> Option<i64> {
    let (y1, m1, mut d1) = serial_to_ymd(start);
    let (y2, m2, mut d2) = serial_to_ymd(end);
    if european {
        d1 = d1.min(30);
        d2 = d2.min(30);
    } else {
        if d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 && d1 == 30 {
            d2 = 30;
        }
    }
    Some((y2 - y1) * 360 + (m2 - m1) * 30 + (d2 - d1))
}

/// The actual/actual basis divisor: the mean length of the years spanned.
fn average_year_length(start: i64, end: i64) -> f64 {
    let (y1, _, _) = serial_to_ymd(start);
    let (y2, _, _) = serial_to_ymd(end);
    let mut total = 0.0;
    for year in y1..=y2 {
        total += if is_leap(year) { 366.0 } else { 365.0 };
    }
    total / ((y2 - y1 + 1) as f64)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// `NETWORKDAYS` (count) and `WORKDAY` (advance), which share their weekend and
/// holiday handling.
fn eval_workdays(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], advance: bool) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, second) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc() as i64, b.trunc()),
        Err(e) => return e,
    };
    let holidays: Vec<i64> = match args.get(2) {
        Some(_) => match flatten_numbers(ev, sheet, &args[2..]) {
            Ok(ns) => ns.into_iter().map(|n| n.trunc() as i64).collect(),
            Err(e) => return Value::Error(e),
        },
        None => Vec::new(),
    };
    let is_workday = |serial: i64| {
        let day = weekday_of(serial);
        day != 0 && day != 6 && !holidays.contains(&serial)
    };

    if advance {
        let mut remaining = second as i64;
        let step = if remaining >= 0 { 1 } else { -1 };
        let mut at = start;
        while remaining != 0 {
            at += step;
            if is_workday(at) {
                remaining -= step;
            }
        }
        return Value::Number(at as f64);
    }
    // NETWORKDAYS counts inclusively at both ends and is symmetric: a reversed
    // pair returns the same magnitude, negated.
    let end = second as i64;
    let (lo, hi) = (start.min(end), start.max(end));
    let count = (lo..=hi).filter(|d| is_workday(*d)).count() as f64;
    Value::Number(if end < start { -count } else { count })
}

// --- Lookup and reference helpers ------------------------------------------

/// `ADDRESS(row, col, [abs], [a1], [sheet])` — build a reference *as text*.
///
/// It returns a string, not a reference: `ADDRESS(1,1)` is `"$A$1"`, and it is
/// `INDIRECT` that turns such a string back into something to read.
fn eval_address(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let mut number = |i: usize, default: f64| -> Result<f64, Value> {
        match args.get(i) {
            Some(a) => ev.eval_expr(sheet, a).as_number().map_err(Value::Error),
            None => Ok(default),
        }
    };
    let row = match number(0, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let col = match number(1, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let abs = match number(2, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    if row < 1 || col < 1 || !(1..=4).contains(&abs) {
        return Value::Error(ErrorValue::Value);
    }
    // 1 both absolute, 2 row absolute, 3 column absolute, 4 neither.
    let (row_abs, col_abs) = match abs {
        1 => (true, true),
        2 => (true, false),
        3 => (false, true),
        _ => (false, false),
    };
    let letters = casual_calc_formula::column_to_letters((col - 1) as u32);
    let mut out = format!(
        "{}{letters}{}{row}",
        if col_abs { "$" } else { "" },
        if row_abs { "$" } else { "" }
    );
    if let Some(arg) = args.get(4) {
        match ev.eval_expr(sheet, arg) {
            Value::Text(name) if !name.is_empty() => out = format!("{name}!{out}"),
            Value::Error(e) => return Value::Error(e),
            _ => {}
        }
    }
    Value::Text(out)
}

/// `AREAS(reference)` — how many areas a reference names.
///
/// Answered from the expression, since the evaluator resolves a reference to
/// its contents before a function sees it. Without union syntax in the parser
/// every reference is a single area.
fn eval_areas(args: &[Expr]) -> Value {
    match args {
        [Expr::Reference(_) | Expr::Range(..) | Expr::StructuredRef { .. }] => Value::Number(1.0),
        [_] => Value::Error(ErrorValue::Value),
        _ => Value::Error(ErrorValue::Value),
    }
}

/// `LOOKUP(value, vector, [result])` — the vector form.
///
/// Always approximate: it assumes the lookup vector is sorted ascending and
/// returns the last entry not greater than the target. There is no exact-match
/// mode, which is exactly why MATCH/VLOOKUP exist.
fn eval_lookup(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let target = ev.eval_expr(sheet, &args[0]);
    if let Value::Error(e) = target {
        return Value::Error(e);
    }
    let Some(lookup) = range_cells(ev, sheet, &args[1]) else {
        return Value::Error(ErrorValue::Value);
    };
    let result = match args.get(2) {
        Some(a) => match range_cells(ev, sheet, a) {
            Some(cells) => Some(cells),
            None => return Value::Error(ErrorValue::Value),
        },
        None => None,
    };

    let mut best: Option<usize> = None;
    for (i, at) in lookup.1.iter().enumerate() {
        let value = ev.eval_cell(lookup.0, *at);
        if matches!(loose_cmp(&value, &target), Some(Ordering::Greater)) {
            break;
        }
        if loose_cmp(&value, &target).is_some() {
            best = Some(i);
        }
    }
    let Some(index) = best else {
        return Value::Error(ErrorValue::Na);
    };
    match result {
        Some((rs, cells)) => match cells.get(index) {
            Some(at) => ev.eval_cell(rs, *at),
            None => Value::Error(ErrorValue::Na),
        },
        None => ev.eval_cell(lookup.0, lookup.1[index]),
    }
}

/// The cells a range expression covers, in row-major order.
fn range_cells(ev: &mut Evaluator<'_>, sheet: usize, expr: &Expr) -> Option<(usize, Vec<CellRef>)> {
    let (target, range) = match expr {
        Expr::Range(a, b) => {
            let target = ev.resolve_sheet(&a.sheet, sheet)?;
            (target, ev.range_bounds(target, a, b))
        }
        Expr::StructuredRef { table, spec } => {
            let (target, range) = ev.resolve_structured(sheet, table.as_deref(), spec)?;
            (
                target,
                (
                    range.start.row,
                    range.start.col,
                    range.end.row,
                    range.end.col,
                ),
            )
        }
        _ => return None,
    };
    let (r0, c0, r1, c1) = range;
    if (r1 - r0 + 1) as u64 * (c1 - c0 + 1) as u64 > MAX_RANGE_CELLS {
        return None;
    }
    let mut out = Vec::new();
    for row in r0..=r1 {
        for col in c0..=c1 {
            out.push(CellRef::new(row, col));
        }
    }
    Some((target, out))
}

/// `INDIRECT(text)` — read the cell a *string* names.
///
/// The reason it is special: the dependency graph cannot see through it, since
/// the target is only known once the string is evaluated. `graph.rs` therefore
/// treats a formula containing INDIRECT as depending on everything, the same
/// treatment a defined name gets — conservative, and never stale.
fn eval_indirect(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    // The A1/R1C1 flag: only A1 is supported, and FALSE asks for R1C1, so it
    // is refused rather than silently answered in the wrong notation.
    if let Some(arg) = args.get(1) {
        match ev.eval_expr(sheet, arg).as_bool() {
            Ok(true) => {}
            Ok(false) => return Value::Error(ErrorValue::Value),
            Err(e) => return Value::Error(e),
        }
    }
    let text = match ev.eval_expr(sheet, &args[0]) {
        Value::Text(t) => t,
        Value::Error(e) => return Value::Error(e),
        other => match other.as_number() {
            Ok(n) => n.to_string(),
            Err(e) => return Value::Error(e),
        },
    };
    // A sheet-qualified target resolves through the same path a written
    // reference does, so `INDIRECT("Sheet2!A1")` behaves like `Sheet2!A1`.
    let (sheet_name, cell) = match text.rsplit_once('!') {
        Some((name, cell)) => (Some(name.trim_matches('\'').to_owned()), cell),
        None => (None, text.as_str()),
    };
    let Some(mut reference) = casual_calc_formula::parse_a1(cell) else {
        // A string that is not a reference is #REF!, which is what Excel shows
        // and is distinguishable from the cell simply being empty.
        return Value::Error(ErrorValue::Ref);
    };
    reference.sheet = sheet_name;
    ev.eval_expr(sheet, &Expr::Reference(reference))
}

/// `OFFSET(reference, rows, cols, [height], [width])`.
///
/// Returns the single cell when the result is 1×1. A larger result is a range,
/// and a range on its own is `#VALUE!` here exactly as `A1:B2` is — it is the
/// aggregate around it that consumes one.
fn eval_offset(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let Expr::Reference(base) = &args[0] else {
        return Value::Error(ErrorValue::Value);
    };
    let number = |ev: &mut Evaluator<'_>, i: usize, default: f64| -> Result<f64, Value> {
        match args.get(i) {
            Some(a) => ev.eval_expr(sheet, a).as_number().map_err(Value::Error),
            None => Ok(default),
        }
    };
    let rows = match number(ev, 1, 0.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let cols = match number(ev, 2, 0.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let height = match number(ev, 3, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    let width = match number(ev, 4, 1.0) {
        Ok(v) => v as i64,
        Err(e) => return e,
    };
    if height != 1 || width != 1 {
        return Value::Error(ErrorValue::Value);
    }
    let row = base.row as i64 + rows;
    let col = base.col as i64 + cols;
    if row < 0 || col < 0 {
        // Off the top or left edge of the grid.
        return Value::Error(ErrorValue::Ref);
    }
    let mut target = base.clone();
    target.row = row as u32;
    target.col = col as u32;
    ev.eval_expr(sheet, &Expr::Reference(target))
}

// --- Text helpers ----------------------------------------------------------

/// `CHAR` and `UNICHAR`.
///
/// They differ in range, not in kind: `CHAR` takes 1..=255 and `UNICHAR` any
/// valid code point. Treating them as the same function would accept `CHAR(955)`
/// and return λ, which Excel refuses.
fn eval_char(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], unicode: bool) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let code = match ev.eval_expr(sheet, arg).as_number() {
        Ok(n) => n.trunc(),
        Err(e) => return Value::Error(e),
    };
    if code < 1.0 || (!unicode && code > 255.0) {
        return Value::Error(ErrorValue::Value);
    }
    match u32::try_from(code as i64).ok().and_then(char::from_u32) {
        Some(ch) => Value::Text(ch.to_string()),
        None => Value::Error(ErrorValue::Value),
    }
}

/// `CODE` and `UNICODE` — the code point of the first character.
fn eval_code(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], unicode: bool) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let text = match ev.eval_expr(sheet, arg) {
        Value::Text(t) => t,
        Value::Error(e) => return Value::Error(e),
        other => match other.as_number() {
            Ok(n) => number_to_text(n),
            Err(e) => return Value::Error(e),
        },
    };
    let Some(ch) = text.chars().next() else {
        // Excel reports #VALUE! for empty text rather than 0.
        return Value::Error(ErrorValue::Value);
    };
    let code = ch as u32;
    if !unicode && code > 255 {
        // CODE is byte-oriented; a character it cannot express is #VALUE!.
        return Value::Error(ErrorValue::Value);
    }
    Value::Number(f64::from(code))
}

/// `CLEAN(text)` — drop the non-printable control characters.
fn eval_clean(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    match ev.eval_expr(sheet, arg) {
        Value::Error(e) => Value::Error(e),
        other => {
            let text = match other {
                Value::Text(t) => t,
                Value::Empty => String::new(),
                v => match v.as_number() {
                    Ok(n) => number_to_text(n),
                    Err(e) => return Value::Error(e),
                },
            };
            Value::Text(text.chars().filter(|c| !c.is_control()).collect())
        }
    }
}

/// `FIXED(number, [decimals], [no_commas])` — fixed-point text.
fn eval_fixed(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let decimals = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n.trunc() as i32,
            Err(e) => return Value::Error(e),
        },
        None => 2,
    };
    let no_commas = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_bool() {
            Ok(b) => b,
            Err(e) => return Value::Error(e),
        },
        None => false,
    };
    // A negative `decimals` rounds to the left of the point: FIXED(1234.5,-2)
    // is "1,200". Clamping it to zero instead would quietly change the answer.
    let (rounded, places) = if decimals < 0 {
        let factor = 10f64.powi(-decimals);
        ((value / factor).round() * factor, 0usize)
    } else {
        (value, decimals as usize)
    };
    let mut text = format!("{rounded:.places$}");
    if !no_commas {
        text = group_thousands(&text);
    }
    Value::Text(text)
}

/// `DOLLAR(number, [decimals])` — like FIXED, always grouped, with a currency
/// symbol and parentheses for negatives, as the accounting format uses.
fn eval_dollar(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let decimals = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n.trunc() as i32,
            Err(e) => return Value::Error(e),
        },
        None => 2,
    };
    let (rounded, places) = if decimals < 0 {
        let factor = 10f64.powi(-decimals);
        ((value / factor).round() * factor, 0usize)
    } else {
        (value, decimals as usize)
    };
    let body = group_thousands(&format!("{:.places$}", rounded.abs()));
    Value::Text(if rounded < 0.0 {
        format!("($={body})").replace("$=", "$")
    } else {
        format!("${body}")
    })
}

/// Insert thousands separators into the integer part of a formatted number.
fn group_thousands(text: &str) -> String {
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text),
    };
    let (int, frac) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    let mut grouped = String::new();
    for (i, ch) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    match frac {
        Some(f) => format!("{sign}{grouped}.{f}"),
        None => format!("{sign}{grouped}"),
    }
}

/// `NUMBERVALUE(text, [decimal], [group])` — parse a number written with
/// explicit separators, rather than guessing at the locale.
fn eval_numbervalue(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]) {
        Value::Text(t) => t,
        Value::Error(e) => return Value::Error(e),
        other => match other.as_number() {
            Ok(n) => return Value::Number(n),
            Err(e) => return Value::Error(e),
        },
    };
    let mut separator = |i: usize, default: char| -> Result<char, Value> {
        match args.get(i) {
            Some(a) => match ev.eval_expr(sheet, a) {
                Value::Text(t) => Ok(t.chars().next().unwrap_or(default)),
                Value::Error(e) => Err(Value::Error(e)),
                _ => Ok(default),
            },
            None => Ok(default),
        }
    };
    let decimal = match separator(1, '.') {
        Ok(c) => c,
        Err(e) => return e,
    };
    let group = match separator(2, ',') {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mut cleaned = String::new();
    for ch in text.chars() {
        if ch == group || ch.is_whitespace() {
            continue;
        }
        cleaned.push(if ch == decimal { '.' } else { ch });
    }
    // A trailing percent scales the result, which is the one piece of
    // interpretation the function does beyond separators.
    let percents = cleaned.chars().rev().take_while(|c| *c == '%').count();
    let body = cleaned.trim_end_matches('%');
    match body.parse::<f64>() {
        Ok(n) => Value::Number(n / 100f64.powi(percents as i32)),
        Err(_) => Value::Error(ErrorValue::Value),
    }
}

// --- Statistics ------------------------------------------------------------

/// Run `f` over the flattened numeric arguments. An empty sample, or an `f`
/// returning `None`, is `#NUM!` — the value a statistic has no meaning for.
fn stat_over(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(&[f64]) -> Option<f64>,
) -> Value {
    match flatten_numbers(ev, sheet, args) {
        Ok(ns) if ns.is_empty() => Value::Error(ErrorValue::Num),
        Ok(ns) => match f(&ns) {
            Some(v) if v.is_finite() => Value::Number(v),
            _ => Value::Error(ErrorValue::Num),
        },
        Err(e) => Value::Error(e),
    }
}

fn mean(ns: &[f64]) -> f64 {
    ns.iter().sum::<f64>() / ns.len() as f64
}

/// Variance, sample (`n-1`) or population (`n`).
///
/// The divisor is the whole difference between VAR and VARP, and using the
/// wrong one gives an answer close enough to pass a glance on any large sample.
fn variance(ns: &[f64], sample: bool) -> Option<f64> {
    let n = ns.len();
    if sample && n < 2 {
        return None;
    }
    let m = mean(ns);
    let sum: f64 = ns.iter().map(|x| (x - m).powi(2)).sum();
    Some(sum / if sample { (n - 1) as f64 } else { n as f64 })
}

/// The most frequent value, or `None` when every value occurs once — Excel
/// reports `#N/A` for that, not the first value.
fn mode_of(ns: &[f64]) -> Option<f64> {
    let mut best: Option<(f64, usize)> = None;
    for candidate in ns {
        let count = ns.iter().filter(|n| *n == candidate).count();
        if count > best.map_or(0, |(_, c)| c) {
            best = Some((*candidate, count));
        }
    }
    best.filter(|(_, count)| *count > 1).map(|(v, _)| v)
}

fn skew_of(ns: &[f64]) -> Option<f64> {
    let n = ns.len();
    if n < 3 {
        return None;
    }
    let m = mean(ns);
    let sd = variance(ns, true)?.sqrt();
    if sd == 0.0 {
        return None;
    }
    let n = n as f64;
    let sum: f64 = ns.iter().map(|x| ((x - m) / sd).powi(3)).sum();
    Some(n / ((n - 1.0) * (n - 2.0)) * sum)
}

fn kurt_of(ns: &[f64]) -> Option<f64> {
    let count = ns.len();
    if count < 4 {
        return None;
    }
    let m = mean(ns);
    let sd = variance(ns, true)?.sqrt();
    if sd == 0.0 {
        return None;
    }
    let n = count as f64;
    let sum: f64 = ns.iter().map(|x| ((x - m) / sd).powi(4)).sum();
    Some(
        n * (n + 1.0) / ((n - 1.0) * (n - 2.0) * (n - 3.0)) * sum
            - 3.0 * (n - 1.0).powi(2) / ((n - 2.0) * (n - 3.0)),
    )
}

/// `PERCENTILE(array, k)`, or `QUARTILE(array, q)` with `q` in 0..=4.
///
/// Linear interpolation between order statistics, which is what Excel's
/// inclusive percentile does; a nearest-rank implementation disagrees on most
/// samples.
fn eval_percentile(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], quartile: bool) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let mut ns = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let k = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => {
            if quartile {
                if !(0.0..=4.0).contains(&n) {
                    return Value::Error(ErrorValue::Num);
                }
                n.trunc() / 4.0
            } else {
                n
            }
        }
        Err(e) => return Value::Error(e),
    };
    if ns.is_empty() || !(0.0..=1.0).contains(&k) {
        return Value::Error(ErrorValue::Num);
    }
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    Value::Number(percentile_of(&ns, k))
}

fn percentile_of(sorted: &[f64], k: f64) -> f64 {
    let position = k * (sorted.len() - 1) as f64;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    if lower == upper {
        return sorted[lower];
    }
    sorted[lower] + (position - lower as f64) * (sorted[upper] - sorted[lower])
}

/// `PERCENTRANK(array, x, [significance])` — the inverse of PERCENTILE.
fn eval_percentrank(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let mut ns = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let x = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    if ns.is_empty() {
        return Value::Error(ErrorValue::Num);
    }
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    if x < ns[0] || x > ns[ns.len() - 1] {
        return Value::Error(ErrorValue::Na);
    }
    let below = ns.iter().filter(|n| **n < x).count() as f64;
    let equal = ns.iter().filter(|n| **n == x).count() as f64;
    let rank = if equal > 0.0 {
        below / (ns.len() - 1) as f64
    } else {
        // Between two observations: interpolate, as Excel does.
        let lower = ns.iter().rev().find(|n| **n < x).copied().unwrap_or(ns[0]);
        let upper = ns.iter().find(|n| **n > x).copied().unwrap_or(x);
        let base = ns.iter().filter(|n| **n < x).count() as f64 - 1.0;
        (base + (x - lower) / (upper - lower)) / (ns.len() - 1) as f64
    };
    let digits = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n.trunc() as i32,
            Err(e) => return Value::Error(e),
        },
        None => 3,
    };
    let factor = 10f64.powi(digits);
    // Truncated, not rounded: PERCENTRANK reports significant digits rather
    // than a rounded value.
    Value::Number((rank * factor).trunc() / factor)
}

/// `TRIMMEAN(array, percent)` — the mean after discarding the extremes.
fn eval_trimmean(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let mut ns = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(ns) => ns,
        Err(e) => return Value::Error(e),
    };
    let percent = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    if ns.is_empty() || !(0.0..1.0).contains(&percent) {
        return Value::Error(ErrorValue::Num);
    }
    ns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    // The count to drop is rounded *down to an even number*, so the same many
    // are discarded from each end.
    let drop = ((ns.len() as f64 * percent / 2.0).floor() as usize) * 2;
    let keep = &ns[drop / 2..ns.len() - drop / 2];
    if keep.is_empty() {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(mean(keep))
}

/// `COUNTBLANK(range)` — cells with no content, which is not the same as cells
/// holding an empty string.
fn eval_countblank(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(arg) = args.first() else {
        return Value::Error(ErrorValue::Value);
    };
    let Some((target, cells)) = range_cells(ev, sheet, arg) else {
        return Value::Error(ErrorValue::Value);
    };
    let count = cells
        .into_iter()
        .filter(|at| matches!(ev.eval_cell(target, *at), Value::Empty))
        .count();
    Value::Number(count as f64)
}

fn eval_standardize(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if sd <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((x - m) / sd)
}

/// Evaluate two ranges of equal length and hand them to `f`.
fn paired(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(&[f64], &[f64]) -> Option<f64>,
) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let xs = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let ys = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    // Mismatched lengths are #N/A rather than being zipped to the shorter one,
    // which would silently answer over part of the data.
    if xs.len() != ys.len() || xs.is_empty() {
        return Value::Error(ErrorValue::Na);
    }
    match f(&xs, &ys) {
        Some(v) if v.is_finite() => Value::Number(v),
        _ => Value::Error(ErrorValue::Div0),
    }
}

fn correlation(xs: &[f64], ys: &[f64]) -> Option<f64> {
    let (mx, my) = (mean(xs), mean(ys));
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for (x, y) in xs.iter().zip(ys) {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
        syy += (y - my).powi(2);
    }
    let denominator = (sxx * syy).sqrt();
    (denominator != 0.0).then(|| sxy / denominator)
}

fn slope(ys: &[f64], xs: &[f64]) -> Option<f64> {
    let (mx, my) = (mean(xs), mean(ys));
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    for (x, y) in xs.iter().zip(ys) {
        sxy += (x - mx) * (y - my);
        sxx += (x - mx).powi(2);
    }
    (sxx != 0.0).then(|| sxy / sxx)
}

fn steyx(ys: &[f64], xs: &[f64]) -> Option<f64> {
    if ys.len() < 3 {
        return None;
    }
    let m = slope(ys, xs)?;
    let b = mean(ys) - m * mean(xs);
    let sse: f64 = xs
        .iter()
        .zip(ys)
        .map(|(x, y)| (y - (m * x + b)).powi(2))
        .sum();
    Some((sse / (ys.len() - 2) as f64).sqrt())
}

/// `FORECAST(x, known_y, known_x)` — the regression line evaluated at `x`.
fn eval_forecast(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let x = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let ys = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let xs = match flatten_numbers(ev, sheet, &args[2..3]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if xs.len() != ys.len() || xs.is_empty() {
        return Value::Error(ErrorValue::Na);
    }
    match slope(&ys, &xs) {
        Some(m) => Value::Number(mean(&ys) - m * mean(&xs) + m * x),
        None => Value::Error(ErrorValue::Div0),
    }
}

fn three_numbers(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Option<Result<[f64; 3], ErrorValue>> {
    if args.len() != 3 {
        return None;
    }
    let mut out = [0.0; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(v) => *slot = v,
            Err(e) => return Some(Err(e)),
        }
    }
    Some(Ok(out))
}

/// The standard normal CDF, via the error function.
fn standard_normal_cdf(z: f64) -> f64 {
    0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2))
}

#[allow(clippy::excessive_precision)]
/// Abramowitz & Stegun 7.1.26 — about 1.5e-7 absolute error, which is finer
/// than the 15 significant digits a spreadsheet displays can distinguish for
/// probabilities.
fn erf(x: f64) -> f64 {
    // Exact at zero by construction. The rational approximation returns about
    // 1e-9 there, which makes NORMSDIST(0) read 0.5000000005 — a wart in the
    // one place every user checks first.
    if x == 0.0 {
        return 0.0;
    }
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    sign * y
}

/// The inverse standard normal CDF (Acklam's rational approximation), refined
/// by one Halley step so the result is accurate to full double precision.
///
/// The coefficients are transcribed at their published precision, which is
/// finer than `f64` holds. They are left as printed so they can be checked
/// against the source rather than against a rounded copy of it.
#[allow(clippy::excessive_precision)]
fn normal_quantile(p: f64) -> f64 {
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_690e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838,
        -2.549_732_539_343_734,
        4.374_664_141_464_968,
        2.938_163_982_698_783,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996,
        3.754_408_661_907_416,
    ];
    const LOW: f64 = 0.024_25;
    let x = if p < LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= 1.0 - LOW {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    // One Halley refinement against the CDF, which lifts the approximation's
    // ~1e-9 relative error to machine precision.
    let e = standard_normal_cdf(x) - p;
    let u = e * (2.0 * std::f64::consts::PI).sqrt() * (x * x / 2.0).exp();
    x - u / (1.0 + x * u / 2.0)
}

/// The Lanczos approximation to `ln Γ(x)`.
///
/// Coefficients transcribed at published precision; see [`normal_quantile`].
#[allow(clippy::excessive_precision)]
fn ln_gamma(x: f64) -> f64 {
    const G: [f64; 9] = [
        0.999_999_999_999_809_93,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection, since the series converges only for x > 0.5.
        return (std::f64::consts::PI / (std::f64::consts::PI * x).sin()).ln() - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = G[0];
    let t = x + 7.5;
    for (i, g) in G.iter().enumerate().skip(1) {
        a += g / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}

fn eval_normdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let Some(v) = three_numbers(ev, sheet, &args[..3]) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let cumulative = match ev.eval_expr(sheet, &args[3]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    if sd <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let z = (x - m) / sd;
    Value::Number(if cumulative {
        standard_normal_cdf(z)
    } else {
        (-z * z / 2.0).exp() / (sd * (2.0 * std::f64::consts::PI).sqrt())
    })
}

fn eval_norminv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [p, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if sd <= 0.0 || p <= 0.0 || p >= 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(m + sd * normal_quantile(p))
}

fn eval_expondist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (x, lambda) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a, b),
        Err(e) => return e,
    };
    let cumulative = match ev.eval_expr(sheet, &args[2]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || lambda <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(if cumulative {
        1.0 - (-lambda * x).exp()
    } else {
        lambda * (-lambda * x).exp()
    })
}

fn eval_poisson(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (x, m) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc(), b),
        Err(e) => return e,
    };
    let cumulative = match ev.eval_expr(sheet, &args[2]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || m < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    // Summed in log space: m^k / k! overflows for quite ordinary means long
    // before the probability itself becomes unrepresentable.
    let term = |k: f64| (-m + k * m.ln() - ln_gamma(k + 1.0)).exp();
    Value::Number(if cumulative {
        (0..=(x as u64)).map(|k| term(k as f64)).sum()
    } else {
        term(x)
    })
}

fn eval_binomdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let Some(v) = three_numbers(ev, sheet, &args[..3]) else {
        return Value::Error(ErrorValue::Value);
    };
    let [s, n, p] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let cumulative = match ev.eval_expr(sheet, &args[3]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    let (s, n) = (s.trunc(), n.trunc());
    if s < 0.0 || s > n || !(0.0..=1.0).contains(&p) {
        return Value::Error(ErrorValue::Num);
    }
    let term = |k: f64| {
        // Log space again: C(n,k) overflows well before the probability does.
        (ln_gamma(n + 1.0) - ln_gamma(k + 1.0) - ln_gamma(n - k + 1.0)
            + k * p.ln()
            + (n - k) * (1.0 - p).ln())
        .exp()
    };
    Value::Number(if cumulative {
        (0..=(s as u64)).map(|k| term(k as f64)).sum()
    } else {
        term(s)
    })
}

/// As [`stat_over`], but over the `A` family's coercion: text counts as 0 and
/// logicals as 0 or 1, rather than being skipped.
fn stat_over_a(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(&[f64]) -> Option<f64>,
) -> Value {
    let mut values = Vec::new();
    for arg in args {
        match range_cells(ev, sheet, arg) {
            Some((target, cells)) => {
                for at in cells {
                    match ev.eval_cell(target, at) {
                        Value::Number(n) => values.push(n),
                        Value::Bool(b) => values.push(if b { 1.0 } else { 0.0 }),
                        // Text is zero, not skipped: that is the whole point of
                        // the A variants, and it drags an average down.
                        Value::Text(_) => values.push(0.0),
                        Value::Error(e) => return Value::Error(e),
                        Value::Empty => {}
                    }
                }
            }
            None => match ev.eval_expr(sheet, arg) {
                Value::Number(n) => values.push(n),
                Value::Bool(b) => values.push(if b { 1.0 } else { 0.0 }),
                Value::Text(_) => values.push(0.0),
                Value::Error(e) => return Value::Error(e),
                Value::Empty => {}
            },
        }
    }
    if values.is_empty() {
        return Value::Error(ErrorValue::Div0);
    }
    match f(&values) {
        Some(v) if v.is_finite() => Value::Number(v),
        _ => Value::Error(ErrorValue::Num),
    }
}

fn eval_lognormdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if x <= 0.0 || sd <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(standard_normal_cdf((x.ln() - m) / sd))
}

fn eval_loginv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [p, m, sd] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if sd <= 0.0 || p <= 0.0 || p >= 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((m + sd * normal_quantile(p)).exp())
}

fn eval_weibull(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let Some(v) = three_numbers(ev, sheet, &args[..3]) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, alpha, beta] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let cumulative = match ev.eval_expr(sheet, &args[3]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || alpha <= 0.0 || beta <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let scaled = (x / beta).powf(alpha);
    Value::Number(if cumulative {
        1.0 - (-scaled).exp()
    } else {
        alpha / beta.powf(alpha) * x.powf(alpha - 1.0) * (-scaled).exp()
    })
}

fn eval_negbinomdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [f, s, p] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (f, s) = (f.trunc(), s.trunc());
    if f < 0.0 || s < 1.0 || !(0.0..=1.0).contains(&p) {
        return Value::Error(ErrorValue::Num);
    }
    let log = ln_gamma(f + s) - ln_gamma(f + 1.0) - ln_gamma(s) + s * p.ln() + f * (1.0 - p).ln();
    Value::Number(log.exp())
}

fn eval_hypgeomdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let mut v = [0.0f64; 4];
    for (i, slot) in v.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(n) => *slot = n.trunc(),
            Err(e) => return Value::Error(e),
        }
    }
    let [k, n, successes, population] = v;
    if k < 0.0 || k > n || k > successes || n > population || successes > population {
        return Value::Error(ErrorValue::Num);
    }
    let log_choose = |a: f64, b: f64| ln_gamma(a + 1.0) - ln_gamma(b + 1.0) - ln_gamma(a - b + 1.0);
    Value::Number(
        (log_choose(successes, k) + log_choose(population - successes, n - k)
            - log_choose(population, n))
        .exp(),
    )
}

/// `CRITBINOM(trials, p, alpha)` — the smallest k whose cumulative binomial
/// probability reaches `alpha`.
fn eval_critbinom(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [trials, p, alpha] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let trials = trials.trunc();
    if trials < 0.0 || !(0.0..=1.0).contains(&p) || !(0.0..=1.0).contains(&alpha) {
        return Value::Error(ErrorValue::Num);
    }
    let mut cumulative = 0.0;
    for k in 0..=(trials as u64) {
        let k = k as f64;
        cumulative += (ln_gamma(trials + 1.0) - ln_gamma(k + 1.0) - ln_gamma(trials - k + 1.0)
            + k * p.ln()
            + (trials - k) * (1.0 - p).ln())
        .exp();
        if cumulative >= alpha {
            return Value::Number(k);
        }
    }
    Value::Number(trials)
}

fn eval_confidence(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [alpha, sd, size] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if alpha <= 0.0 || alpha >= 1.0 || sd <= 0.0 || size < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // Two-tailed, so the quantile is at 1 - alpha/2.
    Value::Number(normal_quantile(1.0 - alpha / 2.0) * sd / size.trunc().sqrt())
}

// --- Engineering: base conversion and bit operations -----------------------

/// The widest value the base conversions accept: ten digits in the source
/// radix, which is the ceiling OOXML sets on all of them.
const BASE_DIGITS: u32 = 10;

/// Parse `text` in `radix`, honouring the two's-complement convention the
/// spreadsheet base functions use.
///
/// A ten-digit value with the top digit set is negative — `1111111111` in
/// binary is -1, not 1023. Parsing it as unsigned is the single most likely
/// mistake here, and it yields a large positive number that looks plausible.
fn parse_in_base(text: &str, radix: u32) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() || text.len() > BASE_DIGITS as usize {
        return None;
    }
    let magnitude = i64::from_str_radix(text, radix).ok()?;
    let width = (radix as f64).log2().round() as u32 * BASE_DIGITS;
    let sign_bit = 1i64 << (width - 1);
    Some(if magnitude >= sign_bit {
        magnitude - (sign_bit << 1)
    } else {
        magnitude
    })
}

/// Format `value` in `radix` with the same two's-complement convention.
fn format_in_base(value: i64, radix: u32, places: Option<usize>) -> Option<String> {
    let width = (radix as f64).log2().round() as u32 * BASE_DIGITS;
    let sign_bit = 1i64 << (width - 1);
    if value >= sign_bit || value < -sign_bit {
        return None;
    }
    let encoded = if value < 0 {
        (value + (sign_bit << 1)) as u64
    } else {
        value as u64
    };
    let digits = match radix {
        2 => format!("{encoded:b}"),
        8 => format!("{encoded:o}"),
        16 => format!("{encoded:X}"),
        _ => return None,
    };
    // A negative value always occupies the full width, so `places` is ignored
    // for it — padding a two's-complement form would change its value.
    if value < 0 {
        return Some(digits);
    }
    match places {
        Some(p) if p < digits.len() => None,
        Some(p) => Some(format!("{digits:0>p$}")),
        None => Some(digits),
    }
}

fn text_arg(ev: &mut Evaluator<'_>, sheet: usize, expr: &Expr) -> Result<String, Value> {
    match ev.eval_expr(sheet, expr) {
        Value::Text(t) => Ok(t),
        Value::Error(e) => Err(Value::Error(e)),
        // A binary literal typed as a number reaches here as one, and its
        // digits are the text we want.
        other => other.as_number().map(number_to_text).map_err(Value::Error),
    }
}

fn base_to_dec(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], radix: u32) -> Value {
    let [arg] = args else {
        return Value::Error(ErrorValue::Value);
    };
    let text = match text_arg(ev, sheet, arg) {
        Ok(t) => t,
        Err(e) => return e,
    };
    match parse_in_base(&text, radix) {
        Some(v) => Value::Number(v as f64),
        None => Value::Error(ErrorValue::Num),
    }
}

fn places_arg(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Result<Option<usize>, Value> {
    match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) if n < 0.0 => Err(Value::Error(ErrorValue::Num)),
            Ok(n) => Ok(Some(n.trunc() as usize)),
            Err(e) => Err(Value::Error(e)),
        },
        None => Ok(None),
    }
}

fn dec_to_base(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], radix: u32) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let value = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    let places = match places_arg(ev, sheet, args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match format_in_base(value, radix, places) {
        Some(text) => Value::Text(text),
        None => Value::Error(ErrorValue::Num),
    }
}

fn base_to_base(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], from: u32, to: u32) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match text_arg(ev, sheet, &args[0]) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let places = match places_arg(ev, sheet, args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let Some(value) = parse_in_base(&text, from) else {
        return Value::Error(ErrorValue::Num);
    };
    match format_in_base(value, to, places) {
        Some(text) => Value::Text(text),
        None => Value::Error(ErrorValue::Num),
    }
}

/// The bitwise operations, which are defined only on non-negative integers
/// below 2^48 — a range that fits `f64` exactly, so the result is never a
/// rounded approximation of the bits asked for.
fn bitwise(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: fn(u64, u64) -> u64) -> Value {
    let [a, b] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = 2f64.powi(48);
    if a < 0.0 || b < 0.0 || a >= limit || b >= limit || a.fract() != 0.0 || b.fract() != 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(f(a as u64, b as u64) as f64)
}

fn bit_shift(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], left: bool) -> Value {
    let [value, shift] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let limit = 2f64.powi(48);
    if value < 0.0 || value >= limit || value.fract() != 0.0 || shift.abs() > 53.0 {
        return Value::Error(ErrorValue::Num);
    }
    // A negative shift reverses the direction, which is why the two functions
    // can share this body.
    let shift = if left { shift } else { -shift };
    let result = if shift >= 0.0 {
        (value as u64) << (shift as u32)
    } else {
        (value as u64) >> ((-shift) as u32)
    };
    if (result as f64) >= limit {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(result as f64)
}

/// `DELTA(a, [b])` — 1 when equal; `GESTEP(a, [step])` — 1 when at or above.
fn eval_delta(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], equality: bool) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let a = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let b = match args.get(1) {
        Some(arg) => match ev.eval_expr(sheet, arg).as_number() {
            Ok(n) => n,
            Err(e) => return Value::Error(e),
        },
        None => 0.0,
    };
    let hit = if equality { a == b } else { a >= b };
    Value::Number(if hit { 1.0 } else { 0.0 })
}

/// `ERF(lower, [upper])` — the error function, or the integral between two
/// bounds when an upper one is given.
fn eval_erf(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let lower = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    match args.get(1) {
        Some(arg) => match ev.eval_expr(sheet, arg).as_number() {
            Ok(upper) => Value::Number(erf(upper) - erf(lower)),
            Err(e) => Value::Error(e),
        },
        None => Value::Number(erf(lower)),
    }
}

// --- Financial -------------------------------------------------------------

/// Read up to `max` numeric arguments, filling absent ones from `defaults`.
fn opt_numbers<const N: usize>(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    required: usize,
    defaults: [f64; N],
) -> Result<[f64; N], Value> {
    if args.len() < required || args.len() > N {
        return Err(Value::Error(ErrorValue::Value));
    }
    let mut out = defaults;
    for (i, slot) in out.iter_mut().enumerate() {
        if let Some(arg) = args.get(i) {
            *slot = ev.eval_expr(sheet, arg).as_number().map_err(Value::Error)?;
        }
    }
    Ok(out)
}

/// `(1 + rate)^nper`, and the annuity factor `((1+r)^n - 1) / r`.
///
/// The zero-rate case is a genuine limit, not an edge case to reject: a
/// no-interest loan is an ordinary thing to model, and `0/0` here would make
/// PMT report an error for it.
fn annuity_factor(rate: f64, nper: f64) -> (f64, f64) {
    if rate == 0.0 {
        return (1.0, nper);
    }
    let growth = (1.0 + rate).powf(nper);
    (growth, (growth - 1.0) / rate)
}

/// `type` is 1 when payments fall at the start of the period, which advances
/// every payment by one period's interest.
fn due_factor(rate: f64, kind: f64) -> f64 {
    if kind != 0.0 { 1.0 + rate } else { 1.0 }
}

fn eval_fv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, nper, pmt, pv, kind] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let (growth, factor) = annuity_factor(rate, nper);
    Value::Number(-(pv * growth + pmt * due_factor(rate, kind) * factor))
}

fn eval_pv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, nper, pmt, fv, kind] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let (growth, factor) = annuity_factor(rate, nper);
    Value::Number(-(fv + pmt * due_factor(rate, kind) * factor) / growth)
}

fn eval_pmt(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, nper, pv, fv, kind] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let (growth, factor) = annuity_factor(rate, nper);
    if factor == 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(-(pv * growth + fv) / (due_factor(rate, kind) * factor))
}

fn eval_nper(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, pmt, pv, fv, kind] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0])
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if rate == 0.0 {
        if pmt == 0.0 {
            return Value::Error(ErrorValue::Num);
        }
        return Value::Number(-(pv + fv) / pmt);
    }
    let adjusted = pmt * due_factor(rate, kind);
    let numerator = adjusted - fv * rate;
    let denominator = pv * rate + adjusted;
    if numerator / denominator <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((numerator / denominator).ln() / (1.0 + rate).ln())
}

/// `RATE` has no closed form, so it is solved numerically.
///
/// Newton from the caller's guess, falling back to bisection when Newton
/// wanders — the derivative is near zero around rate 0, where Newton alone
/// diverges on exactly the ordinary case of a nearly interest-free loan.
fn eval_rate(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [nper, pmt, pv, fv, kind, guess] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 0.0, 0.1]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let residual = |rate: f64| {
        let (growth, factor) = annuity_factor(rate, nper);
        pv * growth + pmt * due_factor(rate, kind) * factor + fv
    };
    let mut rate = guess;
    for _ in 0..64 {
        let f = residual(rate);
        if f.abs() < 1e-10 {
            return Value::Number(rate);
        }
        // Numeric derivative: the analytic one is long and its algebra is a
        // ready source of sign errors that only show as slow convergence.
        let h = 1e-7;
        let slope = (residual(rate + h) - f) / h;
        if slope.abs() < 1e-14 {
            break;
        }
        let next = rate - f / slope;
        if !next.is_finite() {
            break;
        }
        rate = next;
    }
    // Bisection over a wide bracket, which converges wherever a root exists.
    let (mut lo, mut hi) = (-0.999_999, 10.0);
    let (mut flo, fhi) = (residual(lo), residual(hi));
    if flo * fhi > 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let fmid = residual(mid);
        if fmid.abs() < 1e-12 {
            return Value::Number(mid);
        }
        if flo * fmid < 0.0 {
            hi = mid;
        } else {
            lo = mid;
            flo = fmid;
        }
    }
    Value::Number((lo + hi) / 2.0)
}

/// The interest (or principal) part of one payment.
fn eval_ipmt(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], interest: bool) -> Value {
    let [rate, per, nper, pv, fv, kind] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if per < 1.0 || per > nper {
        return Value::Error(ErrorValue::Num);
    }
    let payment = match eval_pmt_values(rate, nper, pv, fv, kind) {
        Some(p) => p,
        None => return Value::Error(ErrorValue::Num),
    };
    // The balance carried into this period, which is what the interest accrues
    // on — computed as the future value of the loan after `per - 1` payments.
    let (growth, factor) = annuity_factor(rate, per - 1.0);
    let balance = pv * growth + payment * due_factor(rate, kind) * factor;
    let mut interest_part = -balance * rate;
    // A payment due at the start of its period accrues no interest for it.
    if kind != 0.0 && per > 1.0 {
        interest_part /= 1.0 + rate;
    }
    if kind != 0.0 && per == 1.0 {
        interest_part = 0.0;
    }
    Value::Number(if interest {
        interest_part
    } else {
        payment - interest_part
    })
}

fn eval_pmt_values(rate: f64, nper: f64, pv: f64, fv: f64, kind: f64) -> Option<f64> {
    let (growth, factor) = annuity_factor(rate, nper);
    let denominator = due_factor(rate, kind) * factor;
    (denominator != 0.0).then(|| -(pv * growth + fv) / denominator)
}

/// `ISPMT` — the interest of a straight-line loan, which is *not* the same as
/// IPMT: the principal repays evenly rather than on an amortization schedule.
fn eval_ispmt(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, per, nper, pv] = match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if nper == 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(pv * rate * (per / nper - 1.0))
}

fn eval_npv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 {
        return Value::Error(ErrorValue::Value);
    }
    let rate = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let flows = match flatten_numbers(ev, sheet, &args[1..]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if rate == -1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // NPV discounts the *first* flow by one period: it treats every value as
    // arriving at the end of a period, unlike XNPV which dates them.
    let total: f64 = flows
        .iter()
        .enumerate()
        .map(|(i, v)| v / (1.0 + rate).powi(i as i32 + 1))
        .sum();
    Value::Number(total)
}

fn npv_at(rate: f64, flows: &[f64]) -> f64 {
    flows
        .iter()
        .enumerate()
        .map(|(i, v)| v / (1.0 + rate).powi(i as i32))
        .sum()
}

fn eval_irr(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let flows = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    // Without both signs there is no root, and a solver would wander until it
    // gave up rather than saying so.
    if !flows.iter().any(|v| *v > 0.0) || !flows.iter().any(|v| *v < 0.0) {
        return Value::Error(ErrorValue::Num);
    }
    match solve_rate(|r| npv_at(r, &flows)) {
        Some(r) => Value::Number(r),
        None => Value::Error(ErrorValue::Num),
    }
}

/// Bisect for a rate where `f` crosses zero, over the range a rate can take.
fn solve_rate(f: impl Fn(f64) -> f64) -> Option<f64> {
    let (mut lo, mut hi) = (-0.999_999, 10.0);
    let (mut flo, fhi) = (f(lo), f(hi));
    if !flo.is_finite() || !fhi.is_finite() || flo * fhi > 0.0 {
        return None;
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let fmid = f(mid);
        if fmid.abs() < 1e-12 {
            return Some(mid);
        }
        if flo * fmid < 0.0 {
            hi = mid;
        } else {
            lo = mid;
            flo = fmid;
        }
    }
    Some((lo + hi) / 2.0)
}

fn eval_mirr(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let flows = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (finance, reinvest) = match pair_of_numbers(ev, sheet, &args[1..3]) {
        Ok([a, b]) => (a, b),
        Err(e) => return e,
    };
    let n = flows.len() as f64;
    if n < 2.0 {
        return Value::Error(ErrorValue::Div0);
    }
    // Negatives discounted at the finance rate, positives compounded at the
    // reinvestment rate — the whole point of MIRR over IRR.
    let negatives: f64 = flows
        .iter()
        .enumerate()
        .filter(|(_, v)| **v < 0.0)
        .map(|(i, v)| v / (1.0 + finance).powi(i as i32))
        .sum();
    let positives: f64 = flows
        .iter()
        .enumerate()
        .filter(|(_, v)| **v > 0.0)
        .map(|(i, v)| v * (1.0 + reinvest).powi((n as i32 - 1) - i as i32))
        .sum();
    if negatives == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    Value::Number((-positives / negatives).powf(1.0 / (n - 1.0)) - 1.0)
}

fn eval_xnpv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let rate = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let (flows, dates) = match dated_flows(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let start = dates[0];
    Value::Number(
        flows
            .iter()
            .zip(&dates)
            .map(|(v, d)| v / (1.0 + rate).powf((d - start) / 365.0))
            .sum(),
    )
}

fn eval_xirr(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let (flows, dates) = match dated_flows(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !flows.iter().any(|v| *v > 0.0) || !flows.iter().any(|v| *v < 0.0) {
        return Value::Error(ErrorValue::Num);
    }
    let start = dates[0];
    let f = |rate: f64| -> f64 {
        flows
            .iter()
            .zip(&dates)
            .map(|(v, d)| v / (1.0 + rate).powf((d - start) / 365.0))
            .sum()
    };
    match solve_rate(f) {
        Some(r) => Value::Number(r),
        None => Value::Error(ErrorValue::Num),
    }
}

/// The `(values, dates)` pair XNPV and XIRR share, validated together.
fn dated_flows(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
) -> Result<(Vec<f64>, Vec<f64>), Value> {
    // XNPV takes the rate first, XIRR does not.
    let offset = usize::from(args.len() == 3 && !matches!(args[0], Expr::Range(..)));
    let flows = flatten_numbers(ev, sheet, &args[offset..offset + 1]).map_err(Value::Error)?;
    let dates = flatten_numbers(ev, sheet, &args[offset + 1..offset + 2]).map_err(Value::Error)?;
    if flows.len() != dates.len() || flows.is_empty() {
        return Err(Value::Error(ErrorValue::Num));
    }
    Ok((flows, dates))
}

fn eval_fvschedule(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let principal = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let schedule = match flatten_numbers(ev, sheet, &args[1..]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    Value::Number(schedule.iter().fold(principal, |acc, r| acc * (1.0 + r)))
}

fn eval_sln(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [cost, salvage, life] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if life == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    Value::Number((cost - salvage) / life)
}

fn eval_syd(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [cost, salvage, life, per] = match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if life <= 0.0 || per < 1.0 || per > life {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((cost - salvage) * (life - per + 1.0) * 2.0 / (life * (life + 1.0)))
}

/// `DB` — fixed-declining balance, whose rate is rounded to three decimals.
///
/// That rounding is in the definition, not an implementation shortcut: leaving
/// it out changes every period's figure by a little, which is exactly the kind
/// of error that survives review.
fn eval_db(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [cost, salvage, life, period, month] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 12.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if cost <= 0.0 || life <= 0.0 || period < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // The rate is rounded to three decimals *by definition*, not as an
    // implementation shortcut: omitting the rounding shifts every period's
    // figure slightly, which is the kind of error that survives review.
    let rate = ((1.0 - (salvage / cost).powf(1.0 / life)) * 1000.0).round() / 1000.0;
    let first = cost * rate * month / 12.0;
    if period == 1.0 {
        return Value::Number(first);
    }
    let mut total = first;
    let mut current = 0.0;
    for _ in 2..=(period as u64) {
        current = (cost - total) * rate;
        total += current;
    }
    // The final period covers only the remaining months of the year.
    if period > life {
        current = (cost - total + current) * rate * (12.0 - month) / 12.0;
    }
    Value::Number(current)
}

/// `DDB` — double-declining balance, never depreciating below the salvage.
fn eval_ddb(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [cost, salvage, life, period, factor] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 2.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if cost < 0.0 || life <= 0.0 || period < 1.0 || factor <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let mut total = 0.0;
    let mut current = 0.0;
    for _ in 1..=(period as u64) {
        current = ((cost - total) * factor / life)
            .min(cost - salvage - total)
            .max(0.0);
        total += current;
    }
    Value::Number(current)
}

/// `EFFECT` and `NOMINAL`, which are inverses of each other.
fn eval_effect(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], to_effective: bool) -> Value {
    let [rate, periods] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let periods = periods.trunc();
    if rate <= 0.0 || periods < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(if to_effective {
        (1.0 + rate / periods).powf(periods) - 1.0
    } else {
        ((1.0 + rate).powf(1.0 / periods) - 1.0) * periods
    })
}

fn eval_rri(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [nper, pv, fv] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if nper <= 0.0 || pv <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((fv / pv).powf(1.0 / nper) - 1.0)
}

fn eval_pduration(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [rate, pv, fv] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if rate <= 0.0 || pv <= 0.0 || fv <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((fv.ln() - pv.ln()) / (1.0 + rate).ln())
}

/// `DOLLARDE` / `DOLLARFR` — prices written as whole units plus a fraction,
/// as bond quotes are.
fn eval_dollar_frac(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    to_decimal: bool,
) -> Value {
    let [value, fraction] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let fraction = fraction.trunc();
    if fraction < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    let whole = value.trunc();
    let rest = value - whole;
    // The fractional part is written in base `fraction` but *positioned* by
    // decimal digits: at 16ths, 1.02 means 1 + 2/16 and 1.15 means 1 + 15/16.
    // So the scale is 10^(digits in `fraction`), not the fraction itself.
    let digits = fraction.log10().floor() + 1.0;
    let scale = 10f64.powf(digits);
    Value::Number(if to_decimal {
        whole + rest * scale / fraction
    } else {
        whole + rest * fraction / scale
    })
}

// --- Complex numbers -------------------------------------------------------
//
// A complex number is *text* in a spreadsheet — "3+4i" — not a value type. So
// every function here parses its arguments and formats its result, and the
// imaginary suffix travels with the value: a workbook using `j` throughout must
// not come back using `i`.

/// A complex number as `(real, imaginary)`. A named pair rather than a struct
/// because every operation below is arithmetic on two floats and a struct would
/// add ceremony without adding meaning.
type Complex = (f64, f64);

/// An operation that can fail — division by zero, in practice.
type ComplexOp1 = fn(Complex) -> Option<Complex>;
/// A two-argument operation that can fail.
type ComplexOp2 = fn(Complex, Complex) -> Option<Complex>;
/// A total two-argument operation, for the folds.
type ComplexFold = fn(Complex, Complex) -> Complex;

/// Parse `"3+4i"`, `"-2.5j"`, `"7"` or `"i"` into `(real, imaginary, suffix)`.
fn parse_complex(text: &str) -> Option<(f64, f64, char)> {
    let t = text.trim();
    if t.is_empty() {
        return Some((0.0, 0.0, 'i'));
    }
    let suffix = if t.ends_with('i') {
        'i'
    } else if t.ends_with('j') {
        'j'
    } else {
        // No suffix at all: a plain real number.
        return t.parse::<f64>().ok().map(|r| (r, 0.0, 'i'));
    };
    let body = &t[..t.len() - 1];
    // Split at the sign that separates the parts, skipping a leading sign and
    // any exponent sign — `1e-3+2i` must not split at the exponent's minus.
    let bytes = body.as_bytes();
    let mut split = None;
    for i in (1..bytes.len()).rev() {
        let c = bytes[i] as char;
        if (c == '+' || c == '-') && !matches!(bytes[i - 1] as char, 'e' | 'E') {
            split = Some(i);
            break;
        }
    }
    match split {
        Some(i) => {
            let real = body[..i].parse::<f64>().ok()?;
            // "3+i" means 3 + 1i, and "3-i" means 3 - 1i: a bare sign is a
            // coefficient of one.
            let imag_text = &body[i..];
            let imag = match imag_text {
                "+" => 1.0,
                "-" => -1.0,
                other => other.parse::<f64>().ok()?,
            };
            Some((real, imag, suffix))
        }
        None => {
            let imag = match body {
                "" | "+" => 1.0,
                "-" => -1.0,
                other => other.parse::<f64>().ok()?,
            };
            Some((0.0, imag, suffix))
        }
    }
}

/// Format `(real, imaginary)` the way Excel writes one: the parts that are zero
/// are omitted, and a unit coefficient is written as a bare `i`.
fn format_complex(real: f64, imag: f64, suffix: char) -> String {
    let n = |v: f64| number_to_text(v);
    if imag == 0.0 {
        return n(real);
    }
    let imag_part = if imag == 1.0 {
        suffix.to_string()
    } else if imag == -1.0 {
        format!("-{suffix}")
    } else {
        format!("{}{suffix}", n(imag))
    };
    if real == 0.0 {
        return imag_part;
    }
    if imag > 0.0 {
        format!("{}+{imag_part}", n(real))
    } else {
        format!("{}{imag_part}", n(real))
    }
}

fn complex_arg(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    expr: &Expr,
) -> Result<(f64, f64, char), Value> {
    let text = match ev.eval_expr(sheet, expr) {
        Value::Text(t) => t,
        Value::Error(e) => return Err(Value::Error(e)),
        other => other
            .as_number()
            .map(number_to_text)
            .map_err(Value::Error)?,
    };
    parse_complex(&text).ok_or(Value::Error(ErrorValue::Num))
}

/// A function of one complex number returning a real.
fn complex_part(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(Complex) -> f64,
) -> Value {
    let [arg] = args else {
        return Value::Error(ErrorValue::Value);
    };
    match complex_arg(ev, sheet, arg) {
        Ok((re, im, _)) => Value::Number(f((re, im))),
        Err(e) => e,
    }
}

/// A function of one complex number returning another.
fn complex_map(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: fn(Complex) -> Complex,
) -> Value {
    let [arg] = args else {
        return Value::Error(ErrorValue::Value);
    };
    match complex_arg(ev, sheet, arg) {
        Ok((re, im, suffix)) => {
            let (r, i) = f((re, im));
            Value::Text(format_complex(r, i, suffix))
        }
        Err(e) => e,
    }
}

/// As [`complex_map`], but the operation can fail (a division by zero).
fn complex_pair_self(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: ComplexOp1) -> Value {
    let [arg] = args else {
        return Value::Error(ErrorValue::Value);
    };
    match complex_arg(ev, sheet, arg) {
        Ok((re, im, suffix)) => match f((re, im)) {
            Some((r, i)) => Value::Text(format_complex(r, i, suffix)),
            None => Value::Error(ErrorValue::Div0),
        },
        Err(e) => e,
    }
}

/// A function of two complex numbers.
fn complex_pair(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], f: ComplexOp2) -> Value {
    let [a, b] = args else {
        return Value::Error(ErrorValue::Value);
    };
    let (ar, ai, suffix) = match complex_arg(ev, sheet, a) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (br, bi, _) = match complex_arg(ev, sheet, b) {
        Ok(v) => v,
        Err(e) => return e,
    };
    match f((ar, ai), (br, bi)) {
        Some((r, i)) => Value::Text(format_complex(r, i, suffix)),
        None => Value::Error(ErrorValue::Div0),
    }
}

/// Fold a variadic list of complex numbers.
fn complex_fold(
    ev: &mut Evaluator<'_>,
    sheet: usize,
    args: &[Expr],
    f: ComplexFold,
    identity: Complex,
) -> Value {
    if args.is_empty() {
        return Value::Error(ErrorValue::Value);
    }
    let mut acc = identity;
    // The first argument's suffix wins, so a sheet written in `j` stays in `j`.
    let mut suffix = 'i';
    for (n, arg) in args.iter().enumerate() {
        match complex_arg(ev, sheet, arg) {
            Ok((re, im, s)) => {
                if n == 0 {
                    suffix = s;
                }
                acc = f(acc, (re, im));
            }
            Err(e) => return e,
        }
    }
    Value::Text(format_complex(acc.0, acc.1, suffix))
}

fn eval_complex(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let [re, im] = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let suffix = match args.get(2) {
        Some(arg) => match ev.eval_expr(sheet, arg) {
            Value::Text(t) => match t.as_str() {
                "i" => 'i',
                "j" => 'j',
                // Only i and j are legal; anything else is a typo that would
                // otherwise produce a value nothing can parse back.
                _ => return Value::Error(ErrorValue::Value),
            },
            Value::Error(e) => return Value::Error(e),
            _ => return Value::Error(ErrorValue::Value),
        },
        None => 'i',
    };
    Value::Text(format_complex(re, im, suffix))
}

fn eval_impower(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let (re, im, suffix) = match complex_arg(ev, sheet, &args[0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let power = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let modulus = re.hypot(im);
    if modulus == 0.0 {
        return Value::Text(format_complex(0.0, 0.0, suffix));
    }
    // De Moivre: (r∠θ)^n = r^n ∠ nθ.
    let arg = im.atan2(re);
    let r = modulus.powf(power);
    let t = arg * power;
    Value::Text(format_complex(r * t.cos(), r * t.sin(), suffix))
}

// --- Incomplete gamma and beta ---------------------------------------------
//
// Every distribution below reduces to one of these two, so they are implemented
// once. Both use the standard series/continued-fraction split: the series
// converges quickly below the distribution's mean and stalls above it, and the
// continued fraction does the opposite. Using either alone is accurate over
// half its range and quietly wrong over the other half.

const GAMMA_ITERATIONS: usize = 300;
const GAMMA_EPSILON: f64 = 1e-15;

/// The regularized lower incomplete gamma `P(a, x)`.
fn gamma_p(a: f64, x: f64) -> f64 {
    if x <= 0.0 || a <= 0.0 {
        return 0.0;
    }
    if x < a + 1.0 {
        // Series representation.
        let mut term = 1.0 / a;
        let mut sum = term;
        let mut n = a;
        for _ in 0..GAMMA_ITERATIONS {
            n += 1.0;
            term *= x / n;
            sum += term;
            if term.abs() < sum.abs() * GAMMA_EPSILON {
                break;
            }
        }
        sum * (-x + a * x.ln() - ln_gamma(a)).exp()
    } else {
        1.0 - gamma_q_cf(a, x)
    }
}

/// The regularized upper incomplete gamma `Q(a, x)`, by continued fraction.
fn gamma_q_cf(a: f64, x: f64) -> f64 {
    let tiny = 1e-300;
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / tiny;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..GAMMA_ITERATIONS {
        let an = -(i as f64) * (i as f64 - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < tiny {
            d = tiny;
        }
        c = b + an / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < GAMMA_EPSILON {
            break;
        }
    }
    h * (-x + a * x.ln() - ln_gamma(a)).exp()
}

/// The regularized incomplete beta `I_x(a, b)`.
fn beta_i(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front =
        (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    // The continued fraction converges only for x below the distribution's
    // mode; above it, the symmetry I_x(a,b) = 1 - I_(1-x)(b,a) moves the
    // argument back into the converging half.
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_cf(a, b, x) / a
    } else {
        1.0 - front * beta_cf(b, a, 1.0 - x) / b
    }
}

fn beta_cf(a: f64, b: f64, x: f64) -> f64 {
    let tiny = 1e-300;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < tiny {
        d = tiny;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..GAMMA_ITERATIONS {
        let m = m as f64;
        let m2 = 2.0 * m;
        // Even step.
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        h *= d * c;
        // Odd step.
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < GAMMA_EPSILON {
            break;
        }
    }
    h
}

/// Invert a monotone CDF by bisection.
///
/// The distributions below have no closed-form inverse, and bisection over a
/// bracket that is grown until it contains the root converges for all of them —
/// where a fixed bracket would silently return its own endpoint for extreme
/// probabilities.
fn invert_cdf(p: f64, cdf: impl Fn(f64) -> f64) -> Option<f64> {
    if !(0.0..=1.0).contains(&p) {
        return None;
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    let mut guard = 0;
    while cdf(hi) < p {
        hi *= 2.0;
        guard += 1;
        if guard > 200 || !hi.is_finite() {
            return None;
        }
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if cdf(mid) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some((lo + hi) / 2.0)
}

/// `CHIDIST` is the **upper** tail, unlike almost every other `*DIST`.
fn eval_chidist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [x, df] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if x < 0.0 || df < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(1.0 - gamma_p(df / 2.0, x / 2.0))
}

fn eval_chiinv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [p, df] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !(0.0..=1.0).contains(&p) || df < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // CHIINV inverts the upper tail, matching CHIDIST.
    match invert_cdf(1.0 - p, |x| gamma_p(df / 2.0, x / 2.0)) {
        Some(v) => Value::Number(v),
        None => Value::Error(ErrorValue::Num),
    }
}

/// The Student's t CDF.
fn t_cdf(t: f64, df: f64) -> f64 {
    let x = df / (df + t * t);
    let half = 0.5 * beta_i(df / 2.0, 0.5, x);
    if t > 0.0 { 1.0 - half } else { half }
}

/// `TDIST(x, df, tails)` — the legacy form, which takes only positive `x` and
/// reports a tail probability rather than a CDF.
fn eval_tdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, df, tails] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || df < 1.0 || !(tails == 1.0 || tails == 2.0) {
        return Value::Error(ErrorValue::Num);
    }
    let upper = 1.0 - t_cdf(x, df);
    Value::Number(upper * tails)
}

fn eval_tinv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let [p, df] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !(0.0..=1.0).contains(&p) || df < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    // TINV is two-tailed, so it inverts against 1 - p/2.
    match invert_cdf(1.0 - p / 2.0, |x| t_cdf(x, df)) {
        Some(v) => Value::Number(v),
        None => Value::Error(ErrorValue::Num),
    }
}

/// `FDIST` is the upper tail, like CHIDIST.
fn eval_fdist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, d1, d2] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || d1 < 1.0 || d2 < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(1.0 - f_cdf(x, d1, d2))
}

fn f_cdf(x: f64, d1: f64, d2: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    beta_i(d1 / 2.0, d2 / 2.0, d1 * x / (d1 * x + d2))
}

fn eval_finv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [p, d1, d2] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if !(0.0..=1.0).contains(&p) || d1 < 1.0 || d2 < 1.0 {
        return Value::Error(ErrorValue::Num);
    }
    match invert_cdf(1.0 - p, |x| f_cdf(x, d1, d2)) {
        Some(v) => Value::Number(v),
        None => Value::Error(ErrorValue::Num),
    }
}

fn eval_gammadist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let Some(v) = three_numbers(ev, sheet, &args[..3]) else {
        return Value::Error(ErrorValue::Value);
    };
    let [x, alpha, beta] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let cumulative = match ev.eval_expr(sheet, &args[3]).as_bool() {
        Ok(b) => b,
        Err(e) => return Value::Error(e),
    };
    if x < 0.0 || alpha <= 0.0 || beta <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(if cumulative {
        gamma_p(alpha, x / beta)
    } else {
        ((alpha - 1.0) * (x / beta).ln() - x / beta - ln_gamma(alpha)).exp() / beta
    })
}

fn eval_gammainv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [p, alpha, beta] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if !(0.0..=1.0).contains(&p) || alpha <= 0.0 || beta <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    match invert_cdf(p, |x| gamma_p(alpha, x / beta)) {
        Some(v) => Value::Number(v),
        None => Value::Error(ErrorValue::Num),
    }
}

fn eval_betadist(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let [x, alpha, beta, lo, hi] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 1.0])
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if alpha <= 0.0 || beta <= 0.0 || hi <= lo || x < lo || x > hi {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(beta_i(alpha, beta, (x - lo) / (hi - lo)))
}

fn eval_betainv(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let [p, alpha, beta, lo, hi] = match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0, 1.0])
    {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !(0.0..=1.0).contains(&p) || alpha <= 0.0 || beta <= 0.0 || hi <= lo {
        return Value::Error(ErrorValue::Num);
    }
    // The beta CDF lives on 0..1, so the bracket is known and bisection is
    // direct rather than needing the growing bracket the others use.
    let (mut a, mut b) = (0.0f64, 1.0f64);
    for _ in 0..200 {
        let mid = (a + b) / 2.0;
        if beta_i(alpha, beta, mid) < p {
            a = mid;
        } else {
            b = mid;
        }
    }
    Value::Number(lo + (hi - lo) * (a + b) / 2.0)
}

// --- Statistical tests -----------------------------------------------------

fn eval_ztest(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 || args.len() > 3 {
        return Value::Error(ErrorValue::Value);
    }
    let sample = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let x = match ev.eval_expr(sheet, &args[1]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    if sample.is_empty() {
        return Value::Error(ErrorValue::Num);
    }
    // Without a stated sigma the sample's own standard deviation stands in,
    // which is what makes ZTEST usable on a sample rather than a population.
    let sigma = match args.get(2) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n,
            Err(e) => return Value::Error(e),
        },
        None => match variance(&sample, true) {
            Some(v) => v.sqrt(),
            None => return Value::Error(ErrorValue::Div0),
        },
    };
    if sigma <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    let z = (mean(&sample) - x) / (sigma / (sample.len() as f64).sqrt());
    // One-tailed, upper: ZTEST reports the probability of a value this high.
    Value::Number(1.0 - standard_normal_cdf(z))
}

fn eval_ttest(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 4 {
        return Value::Error(ErrorValue::Value);
    }
    let xs = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let ys = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (tails, kind) = match pair_of_numbers(ev, sheet, &args[2..4]) {
        Ok([a, b]) => (a, b),
        Err(e) => return e,
    };
    if !(tails == 1.0 || tails == 2.0) || !(1.0..=3.0).contains(&kind) {
        return Value::Error(ErrorValue::Num);
    }
    let (t, df) = match kind as i32 {
        // Paired: the test is on the differences, so the samples must line up.
        1 => {
            if xs.len() != ys.len() || xs.len() < 2 {
                return Value::Error(ErrorValue::Na);
            }
            let diffs: Vec<f64> = xs.iter().zip(&ys).map(|(x, y)| x - y).collect();
            let sd = match variance(&diffs, true) {
                Some(v) => v.sqrt(),
                None => return Value::Error(ErrorValue::Div0),
            };
            if sd == 0.0 {
                return Value::Error(ErrorValue::Div0);
            }
            let n = diffs.len() as f64;
            (mean(&diffs) / (sd / n.sqrt()), n - 1.0)
        }
        // Equal variance: pooled.
        2 => {
            let (n1, n2) = (xs.len() as f64, ys.len() as f64);
            if n1 < 2.0 || n2 < 2.0 {
                return Value::Error(ErrorValue::Div0);
            }
            let (v1, v2) = (variance(&xs, true).unwrap(), variance(&ys, true).unwrap());
            let pooled = ((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / (n1 + n2 - 2.0);
            let se = (pooled * (1.0 / n1 + 1.0 / n2)).sqrt();
            if se == 0.0 {
                return Value::Error(ErrorValue::Div0);
            }
            ((mean(&xs) - mean(&ys)) / se, n1 + n2 - 2.0)
        }
        // Unequal variance: Welch, whose degrees of freedom are not an integer.
        _ => {
            let (n1, n2) = (xs.len() as f64, ys.len() as f64);
            if n1 < 2.0 || n2 < 2.0 {
                return Value::Error(ErrorValue::Div0);
            }
            let (v1, v2) = (variance(&xs, true).unwrap(), variance(&ys, true).unwrap());
            let se2 = v1 / n1 + v2 / n2;
            if se2 == 0.0 {
                return Value::Error(ErrorValue::Div0);
            }
            let df = se2 * se2 / ((v1 / n1).powi(2) / (n1 - 1.0) + (v2 / n2).powi(2) / (n2 - 1.0));
            ((mean(&xs) - mean(&ys)) / se2.sqrt(), df)
        }
    };
    Value::Number((1.0 - t_cdf(t.abs(), df)) * tails)
}

fn eval_ftest(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let xs = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let ys = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let (Some(v1), Some(v2)) = (variance(&xs, true), variance(&ys, true)) else {
        return Value::Error(ErrorValue::Div0);
    };
    if v1 == 0.0 || v2 == 0.0 {
        return Value::Error(ErrorValue::Div0);
    }
    // The larger variance goes on top so the ratio is ≥ 1 and the tail is the
    // upper one; the other order gives the complement.
    let (hi, lo, dfh, dfl) = if v1 > v2 {
        (v1, v2, xs.len() as f64 - 1.0, ys.len() as f64 - 1.0)
    } else {
        (v2, v1, ys.len() as f64 - 1.0, xs.len() as f64 - 1.0)
    };
    Value::Number(2.0 * (1.0 - f_cdf(hi / lo, dfh, dfl)))
}

fn eval_chitest(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 2 {
        return Value::Error(ErrorValue::Value);
    }
    let actual = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let expected = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if actual.len() != expected.len() || actual.is_empty() {
        return Value::Error(ErrorValue::Na);
    }
    let mut chi = 0.0;
    for (a, e) in actual.iter().zip(&expected) {
        if *e == 0.0 {
            return Value::Error(ErrorValue::Div0);
        }
        chi += (a - e).powi(2) / e;
    }
    let df = (actual.len() - 1) as f64;
    Value::Number(1.0 - gamma_p(df / 2.0, chi / 2.0))
}

fn eval_prob(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let xs = match flatten_numbers(ev, sheet, &args[..1]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let ps = match flatten_numbers(ev, sheet, &args[1..2]) {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    if xs.len() != ps.len() || xs.is_empty() {
        return Value::Error(ErrorValue::Na);
    }
    // The probabilities must be a distribution; Excel refuses otherwise rather
    // than normalizing, since a list that does not sum to 1 is a data error.
    let total: f64 = ps.iter().sum();
    if (total - 1.0).abs() > 1e-9 || ps.iter().any(|p| *p <= 0.0 || *p > 1.0) {
        return Value::Error(ErrorValue::Num);
    }
    let lower = match ev.eval_expr(sheet, &args[2]).as_number() {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let upper = match args.get(3) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(n) => n,
            Err(e) => return Value::Error(e),
        },
        None => lower,
    };
    let (lo, hi) = (lower.min(upper), lower.max(upper));
    Value::Number(
        xs.iter()
            .zip(&ps)
            .filter(|(x, _)| **x >= lo && **x <= hi)
            .map(|(_, p)| p)
            .sum(),
    )
}

/// `SUBTOTAL(fn, ranges…)` — the aggregate a table's totals row uses.
///
/// Codes 1..11 include manually hidden rows; 101..111 exclude them. The
/// distinction is the whole point of the function: a filtered list must not
/// report a total that includes what is hidden.
fn eval_subtotal(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 2 {
        return Value::Error(ErrorValue::Value);
    }
    let code = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(n) => n.trunc() as i32,
        Err(e) => return Value::Error(e),
    };
    let ignore_hidden = code > 100;
    let op = if ignore_hidden { code - 100 } else { code };
    if !(1..=11).contains(&op) {
        return Value::Error(ErrorValue::Value);
    }
    // Gather the values, skipping hidden rows for the 100-series.
    let mut values = Vec::new();
    for arg in &args[1..] {
        match range_cells(ev, sheet, arg) {
            Some((target, cells)) => {
                for at in cells {
                    if ignore_hidden {
                        let hidden = ev
                            .workbook()
                            .sheets
                            .get(target)
                            .is_some_and(|sh| sh.is_row_hidden(at.row));
                        if hidden {
                            continue;
                        }
                    }
                    match ev.eval_cell(target, at) {
                        Value::Number(n) => values.push(n),
                        Value::Error(e) => return Value::Error(e),
                        _ => {}
                    }
                }
            }
            None => match ev.eval_expr(sheet, arg) {
                Value::Number(n) => values.push(n),
                Value::Error(e) => return Value::Error(e),
                _ => {}
            },
        }
    }
    if values.is_empty() && op != 2 && op != 3 {
        return Value::Error(ErrorValue::Div0);
    }
    Value::Number(match op {
        1 => mean(&values),
        2 | 3 => values.len() as f64,
        4 => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        5 => values.iter().copied().fold(f64::INFINITY, f64::min),
        6 => values.iter().product(),
        7 => match variance(&values, true) {
            Some(v) => v.sqrt(),
            None => return Value::Error(ErrorValue::Div0),
        },
        8 => match variance(&values, false) {
            Some(v) => v.sqrt(),
            None => return Value::Error(ErrorValue::Div0),
        },
        9 => values.iter().sum(),
        10 => match variance(&values, true) {
            Some(v) => v,
            None => return Value::Error(ErrorValue::Div0),
        },
        _ => match variance(&values, false) {
            Some(v) => v,
            None => return Value::Error(ErrorValue::Div0),
        },
    })
}

/// `ROMAN(number, [form])` — classic form only; the four "concise" forms
/// differ in how they abbreviate and are not modelled, so a non-zero form is
/// refused rather than silently answered in the classic one.
fn eval_roman(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let n = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(v) => v.trunc() as i64,
        Err(e) => return Value::Error(e),
    };
    if let Some(a) = args.get(1) {
        match ev.eval_expr(sheet, a).as_number() {
            Ok(f) if f != 0.0 => return Value::Error(ErrorValue::Value),
            Ok(_) => {}
            Err(e) => return Value::Error(e),
        }
    }
    if !(0..=3999).contains(&n) {
        return Value::Error(ErrorValue::Value);
    }
    const TABLE: [(i64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut left = n;
    let mut out = String::new();
    for (value, glyph) in TABLE {
        while left >= value {
            out.push_str(glyph);
            left -= value;
        }
    }
    Value::Text(out)
}

/// `ISO.CEILING` and `ECMA.CEILING`. They agree on positives and differ on
/// negatives: ISO rounds toward positive infinity, ECMA away from zero.
fn eval_ceiling_variant(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], iso: bool) -> Value {
    if args.is_empty() || args.len() > 2 {
        return Value::Error(ErrorValue::Value);
    }
    let n = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let step = match args.get(1) {
        Some(a) => match ev.eval_expr(sheet, a).as_number() {
            Ok(v) => v,
            Err(e) => return Value::Error(e),
        },
        None => 1.0,
    };
    if step == 0.0 {
        return Value::Number(0.0);
    }
    let step = step.abs();
    Value::Number(if iso || n >= 0.0 {
        (n / step).ceil() * step
    } else {
        -((-n / step).ceil() * step)
    })
}

/// `CUMIPMT` / `CUMPRINC` — the interest or principal paid across a span of
/// periods, summed from the per-period figures so the two always agree with
/// PMT rather than being derived independently.
fn eval_cumulative(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], interest: bool) -> Value {
    if args.len() != 6 {
        return Value::Error(ErrorValue::Value);
    }
    let mut v = [0.0f64; 6];
    for (i, slot) in v.iter_mut().enumerate() {
        match ev.eval_expr(sheet, &args[i]).as_number() {
            Ok(n) => *slot = n,
            Err(e) => return Value::Error(e),
        }
    }
    let [rate, nper, pv, start, end, kind] = v;
    if rate <= 0.0 || nper <= 0.0 || pv <= 0.0 || start < 1.0 || end < start || end > nper {
        return Value::Error(ErrorValue::Num);
    }
    let Some(payment) = eval_pmt_values(rate, nper, pv, 0.0, kind) else {
        return Value::Error(ErrorValue::Num);
    };
    let mut total = 0.0;
    for per in (start as u64)..=(end as u64) {
        let per = per as f64;
        let (growth, factor) = annuity_factor(rate, per - 1.0);
        let balance = pv * growth + payment * due_factor(rate, kind) * factor;
        let mut part = -balance * rate;
        if kind != 0.0 {
            part = if per == 1.0 { 0.0 } else { part / (1.0 + rate) };
        }
        total += if interest { part } else { payment - part };
    }
    Value::Number(total)
}

/// `DISC` — the discount rate implied by a price.
fn eval_disc(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 4 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let [settle, mature, price, redemption, basis] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if price <= 0.0 || redemption <= 0.0 || mature <= settle {
        return Value::Error(ErrorValue::Num);
    }
    let frac = year_fraction(settle, mature, basis as i64);
    if frac <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number((redemption - price) / redemption / frac)
}

/// `INTRATE` and `RECEIVED`, which invert each other.
fn eval_intrate(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], received: bool) -> Value {
    if args.len() < 4 || args.len() > 5 {
        return Value::Error(ErrorValue::Value);
    }
    let [settle, mature, investment, other, basis] =
        match opt_numbers(ev, sheet, args, 4, [0.0, 0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    if investment <= 0.0 || mature <= settle {
        return Value::Error(ErrorValue::Num);
    }
    let frac = year_fraction(settle, mature, basis as i64);
    if frac <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    if received {
        let denominator = 1.0 - other * frac;
        if denominator == 0.0 {
            return Value::Error(ErrorValue::Num);
        }
        return Value::Number(investment / denominator);
    }
    Value::Number((other - investment) / investment / frac)
}

/// The three Treasury-bill functions, which all use the 360-day actual basis
/// the bill market quotes on.
fn eval_tbill(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], which: u8) -> Value {
    let Some(v) = three_numbers(ev, sheet, args) else {
        return Value::Error(ErrorValue::Value);
    };
    let [settle, mature, third] = match v {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let days = mature.trunc() - settle.trunc();
    // A bill runs at most a year; beyond that the quoting convention does not
    // apply and Excel refuses rather than extrapolating.
    if days <= 0.0 || days > 366.0 {
        return Value::Error(ErrorValue::Num);
    }
    match which {
        0 => {
            if third <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(100.0 * (1.0 - third * days / 360.0))
        }
        1 => {
            if third <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number((100.0 - third) / third * (360.0 / days))
        }
        _ => {
            if third <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(365.0 * third / (360.0 - third * days))
        }
    }
}

/// The year fraction between two serials on an OOXML day-count basis.
fn year_fraction(start: f64, end: f64, basis: i64) -> f64 {
    let (a, b) = (start.trunc() as i64, end.trunc() as i64);
    match basis {
        0 => eval_days360_serials(a, b, false).unwrap_or(0) as f64 / 360.0,
        1 => (b - a) as f64 / average_year_length(a, b),
        2 => (b - a) as f64 / 360.0,
        3 => (b - a) as f64 / 365.0,
        4 => eval_days360_serials(a, b, true).unwrap_or(0) as f64 / 360.0,
        _ => 0.0,
    }
}

/// The `D` functions: an aggregate over the rows of a table that satisfy a
/// criteria block.
///
/// All twelve are one shape — `Dxxx(database, field, criteria)` — differing
/// only in what they do with the picked column, so they share everything up to
/// that point. Writing them separately is how twelve copies of the criteria
/// rules drift apart.
///
/// The criteria block is the part worth stating: its first row names fields,
/// each following row is a set of conditions, conditions **across a row are
/// AND** and **rows are OR**. An empty criteria cell is not a condition at all
/// — reading it as "equals blank" would exclude every row.
fn eval_database(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    if args.len() != 3 {
        return Value::Error(ErrorValue::Value);
    }
    let db = match eval_range_2d(ev, sheet, &args[0]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    let crit = match eval_range_2d(ev, sheet, &args[2]) {
        Ok(g) => g,
        Err(e) => return Value::Error(e),
    };
    // A table with only a header row has no rows to aggregate, and a criteria
    // block with only a header row selects everything.
    if db.rows < 1 || db.cols == 0 || crit.rows < 1 || crit.cols == 0 {
        return Value::Error(ErrorValue::Value);
    }

    let header = |g: &Grid, c: usize| g.get(0, c).as_text().unwrap_or_default().trim().to_owned();
    let db_headers: Vec<String> = (0..db.cols).map(|c| header(&db, c)).collect();

    // `field` is a column name, a 1-based index, or a reference to a header
    // cell — Excel accepts all three, and a file written by someone else will
    // use whichever they preferred.
    let field_value = ev.eval_expr(sheet, &args[1]);
    let field_col: Option<usize> = match &field_value {
        Value::Number(n) => {
            let i = *n as i64;
            if i >= 1 && (i as usize) <= db.cols {
                Some(i as usize - 1)
            } else {
                None
            }
        }
        other => {
            let want = other.as_text().unwrap_or_default();
            let want = want.trim();
            db_headers.iter().position(|h| h.eq_ignore_ascii_case(want))
        }
    };
    // DCOUNTA is the one that allows an absent field: it then counts rows.
    let counting_rows = field_col.is_none() && name == "DCOUNTA";
    if field_col.is_none() && !counting_rows {
        return Value::Error(ErrorValue::Value);
    }

    let mut picked: Vec<Value> = Vec::new();
    for r in 1..db.rows {
        let mut any_row_matched = false;
        for cr in 1..crit.rows {
            let mut all = true;
            let mut had_condition = false;
            for cc in 0..crit.cols {
                let cell = crit.get(cr, cc);
                let text = cell.as_text().unwrap_or_default();
                if text.trim().is_empty() {
                    continue; // not a condition
                }
                had_condition = true;
                let Some(col) = db_headers
                    .iter()
                    .position(|h| h.eq_ignore_ascii_case(header(&crit, cc).trim()))
                else {
                    // A criteria column naming no field cannot be satisfied.
                    all = false;
                    break;
                };
                let (op, operand) = parse_criteria(cell);
                if !criterion_matches(db.get(r, col), op, &operand) {
                    all = false;
                    break;
                }
            }
            // A criteria row with no conditions at all matches everything,
            // which is what an empty row under the headers means.
            if all && (had_condition || crit.cols > 0) {
                any_row_matched = true;
                break;
            }
        }
        if any_row_matched {
            picked.push(if counting_rows {
                Value::Number(1.0)
            } else {
                db.get(r, field_col.expect("checked")).clone()
            });
        }
    }

    let numbers: Vec<f64> = picked
        .iter()
        .filter_map(|v| match v {
            Value::Number(n) => Some(*n),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        })
        .collect();

    match name {
        // DCOUNT counts numbers; DCOUNTA counts anything that is not blank.
        // The pair is the same distinction as COUNT and COUNTA, and swapping
        // them silently changes what a report totals.
        "DCOUNT" => Value::Number(numbers.len() as f64),
        "DCOUNTA" => {
            Value::Number(picked.iter().filter(|v| !matches!(v, Value::Empty)).count() as f64)
        }
        "DGET" => match picked.len() {
            // Excel's own answers: nothing matched is #VALUE!, more than one
            // match is #NUM!. Returning the first would be a plausible wrong
            // answer, which is the worst kind.
            0 => Value::Error(ErrorValue::Value),
            1 => picked.into_iter().next().expect("one"),
            _ => Value::Error(ErrorValue::Num),
        },
        _ if numbers.is_empty() => match name {
            "DSUM" | "DPRODUCT" => Value::Number(0.0),
            _ => Value::Error(ErrorValue::Div0),
        },
        "DSUM" => Value::Number(numbers.iter().sum()),
        "DPRODUCT" => Value::Number(numbers.iter().product()),
        "DAVERAGE" => Value::Number(numbers.iter().sum::<f64>() / numbers.len() as f64),
        "DMAX" => Value::Number(numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        "DMIN" => Value::Number(numbers.iter().copied().fold(f64::INFINITY, f64::min)),
        "DVAR" | "DSTDEV" => {
            // Sample statistics need two points; one has no spread to measure.
            if numbers.len() < 2 {
                return Value::Error(ErrorValue::Div0);
            }
            let mean = numbers.iter().sum::<f64>() / numbers.len() as f64;
            let var = numbers.iter().map(|n| (n - mean).powi(2)).sum::<f64>()
                / (numbers.len() - 1) as f64;
            Value::Number(if name == "DVAR" { var } else { var.sqrt() })
        }
        "DVARP" | "DSTDEVP" => {
            let mean = numbers.iter().sum::<f64>() / numbers.len() as f64;
            let var =
                numbers.iter().map(|n| (n - mean).powi(2)).sum::<f64>() / numbers.len() as f64;
            Value::Number(if name == "DVARP" { var } else { var.sqrt() })
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// `ymd_to_serial` for tests in sibling modules, which cannot see a private fn.
#[cfg(test)]
pub(crate) fn ymd_to_serial_for_test(y: i64, m: i64, d: i64) -> i64 {
    ymd_to_serial(y, m, d)
}

/// The coupon period bracketing `settlement`, as `(previous, next)` serials.
///
/// Coupons are counted **backwards from maturity**, not forwards from issue —
/// a bond's last payment lands on its maturity date, and stepping forwards from
/// an assumed start puts every date a few days out whenever the month lengths
/// differ. Excel counts back, and so does this.
///
/// The day-of-month is taken from maturity and clamped to each month's length,
/// so a bond maturing on the 31st pays on the 30th in a 30-day month and comes
/// back to the 31st afterwards, rather than drifting earlier every period.
fn coupon_period(settlement: i64, maturity: i64, frequency: i64) -> Option<(i64, i64)> {
    if !matches!(frequency, 1 | 2 | 4) || settlement >= maturity {
        return None;
    }
    let months = 12 / frequency;
    let (my, mm, md) = serial_to_ymd(maturity);
    // Step back a period at a time until the date is at or before settlement.
    // Bounded by the periods in a century, so a nonsensical pair cannot spin.
    let step = |k: i64| -> i64 {
        let total = my * 12 + (mm - 1) - k * months;
        let (y, m) = (total.div_euclid(12), total.rem_euclid(12) + 1);
        let last = days_in_month(y, m);
        ymd_to_serial(y, m, md.min(last))
    };
    let mut k = 0;
    while k < 1200 {
        let date = step(k);
        if date <= settlement {
            return Some((date, step(k - 1)));
        }
        k += 1;
    }
    None
}

/// The six `COUP*` functions, which all answer questions about the coupon
/// schedule and therefore all derive from the same one.
///
/// On bases 0 and 4 (the 30/360 conventions) a coupon period is 360/frequency
/// days *by definition* — the whole point of a 30/360 basis is that every
/// period is the same length — so the period length is not measured from the
/// calendar. Measuring it would make COUPDAYS disagree with COUPDAYBS +
/// COUPDAYSNC, which must sum to it.
fn eval_coupon(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    if args.len() < 3 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let [settle, mature, frequency, basis] =
        match opt_numbers(ev, sheet, args, 3, [0.0, 0.0, 0.0, 0.0]) {
            Ok(v) => v,
            Err(e) => return e,
        };
    let basis = basis as i64;
    if !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    let (settle, mature) = (settle.trunc() as i64, mature.trunc() as i64);
    let Some((prev, next)) = coupon_period(settle, mature, frequency as i64) else {
        return Value::Error(ErrorValue::Num);
    };
    let freq = frequency as i64;

    // 30/360 bases define the period; the others measure it.
    let period_days = |a: i64, b: i64| -> f64 {
        match basis {
            0 => eval_days360_serials(a, b, false).unwrap_or(0) as f64,
            4 => eval_days360_serials(a, b, true).unwrap_or(0) as f64,
            _ => (b - a) as f64,
        }
    };

    match name {
        "COUPPCD" => Value::Number(prev as f64),
        "COUPNCD" => Value::Number(next as f64),
        "COUPDAYBS" => Value::Number(period_days(prev, settle)),
        "COUPDAYSNC" => Value::Number(period_days(settle, next)),
        "COUPDAYS" => Value::Number(match basis {
            0 | 4 => 360.0 / freq as f64,
            // Basis 1 measures the actual period; 2 and 3 use their fixed year
            // divided by the frequency, which is what Excel reports.
            1 => (next - prev) as f64,
            2 => 360.0 / freq as f64,
            _ => 365.0 / freq as f64,
        }),
        "COUPNUM" => {
            // Whole periods from settlement to maturity, counting the one that
            // ends at `next`.
            let (my, mm, _) = serial_to_ymd(mature);
            let (ny, nm, _) = serial_to_ymd(next);
            let months = (my * 12 + mm) - (ny * 12 + nm);
            Value::Number((months / (12 / freq)) as f64 + 1.0)
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// The coupon-schedule quantities every bond formula needs: number of periods,
/// and the settlement's position inside its coupon period.
///
/// Returned together because they must come from one schedule — deriving `n`
/// from one calculation and `dsc/e` from another is how a price and a yield
/// stop being inverses of each other.
fn bond_terms(settle: i64, mature: i64, freq: i64, basis: i64) -> Option<(f64, f64, f64)> {
    let (prev, next) = coupon_period(settle, mature, freq)?;
    let period = |a: i64, b: i64| -> f64 {
        match basis {
            0 => eval_days360_serials(a, b, false).unwrap_or(0) as f64,
            4 => eval_days360_serials(a, b, true).unwrap_or(0) as f64,
            _ => (b - a) as f64,
        }
    };
    let e = match basis {
        0 | 2 | 4 => 360.0 / freq as f64,
        1 => (next - prev) as f64,
        _ => 365.0 / freq as f64,
    };
    if e <= 0.0 {
        return None;
    }
    let (my, mm, _) = serial_to_ymd(mature);
    let (ny, nm, _) = serial_to_ymd(next);
    let n = ((my * 12 + mm) - (ny * 12 + nm)) / (12 / freq) + 1;
    // `a/e` is how far into the period settlement sits — the accrued fraction.
    Some((n as f64, period(settle, next) / e, period(prev, settle) / e))
}

/// The clean price of a bond per 100 face, given a yield.
///
/// The last term is the accrued interest: a buyer settling mid-period pays the
/// seller for the days they held it, and the *clean* price is what is quoted.
/// Omitting it prices the bond as though coupons only ever land on settlement.
fn bond_price(
    rate: f64,
    yld: f64,
    redemption: f64,
    freq: f64,
    n: f64,
    dsc_e: f64,
    a_e: f64,
) -> f64 {
    let coupon = 100.0 * rate / freq;
    let k = 1.0 + yld / freq;
    let mut price = redemption / k.powf(n - 1.0 + dsc_e);
    for i in 1..=(n as i64) {
        price += coupon / k.powf(i as f64 - 1.0 + dsc_e);
    }
    price - coupon * a_e
}

/// The bond functions that need the coupon schedule: PRICE, YIELD, DURATION,
/// MDURATION.
///
/// `YIELD` has no closed form, so it is solved numerically against `bond_price`
/// — the same function `PRICE` uses, which is what makes the two exact
/// inverses rather than approximately so.
fn eval_bond(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    let wants = if matches!(name, "PRICE" | "YIELD") {
        6
    } else {
        5
    };
    if args.len() < wants || args.len() > wants + 1 {
        return Value::Error(ErrorValue::Value);
    }
    let nums = match opt_numbers(ev, sheet, args, wants, [0.0; 7]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (settle, mature) = (nums[0].trunc() as i64, nums[1].trunc() as i64);
    let (rate, third) = (nums[2], nums[3]);
    let (redemption, freq, basis) = if wants == 6 {
        (nums[4], nums[5], nums[6] as i64)
    } else {
        (100.0, nums[4], nums[5] as i64)
    };
    if rate < 0.0 || freq <= 0.0 || !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    let Some((n, dsc_e, a_e)) = bond_terms(settle, mature, freq as i64, basis) else {
        return Value::Error(ErrorValue::Num);
    };

    match name {
        "PRICE" => {
            if third < 0.0 || redemption <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(bond_price(rate, third, redemption, freq, n, dsc_e, a_e))
        }
        "YIELD" => {
            let price = third;
            if price <= 0.0 || redemption <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            // Bisection over a bracket wide enough for any real bond. Slower
            // than Newton and immune to the derivative blowing up near zero
            // yield, which is where a bond priced at par sits.
            let f = |y: f64| bond_price(rate, y, redemption, freq, n, dsc_e, a_e) - price;
            let (mut lo, mut hi) = (-0.99, 10.0);
            if f(lo) * f(hi) > 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            for _ in 0..200 {
                let mid = (lo + hi) / 2.0;
                if f(lo) * f(mid) <= 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            Value::Number((lo + hi) / 2.0)
        }
        "DURATION" | "MDURATION" => {
            let yld = third;
            let k = 1.0 + yld / freq;
            let coupon = 100.0 * rate / freq;
            let (mut pv_sum, mut weighted) = (0.0, 0.0);
            for i in 1..=(n as i64) {
                let periods = i as f64 - 1.0 + dsc_e;
                let cash = coupon + if i as f64 == n { 100.0 } else { 0.0 };
                let pv = cash / k.powf(periods);
                pv_sum += pv;
                weighted += pv * periods / freq;
            }
            if pv_sum == 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            let macaulay = weighted / pv_sum;
            Value::Number(if name == "DURATION" {
                macaulay
            } else {
                // Modified duration discounts Macaulay by one period's yield —
                // it answers "how much does the price move", not "when is the
                // money".
                macaulay / k
            })
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// The bond functions that need no coupon schedule, because the instrument has
/// no coupons or pays only at maturity.
fn eval_bond_simple(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    let wants = match name {
        "ACCRINTM" => 4,
        "PRICEDISC" | "YIELDDISC" => 4,
        _ => 5, // PRICEMAT, YIELDMAT
    };
    if args.len() < wants || args.len() > wants + 1 {
        return Value::Error(ErrorValue::Value);
    }
    let v = match opt_numbers(ev, sheet, args, wants, [0.0; 6]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let basis = v[wants] as i64;
    if !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    match name {
        "ACCRINTM" => {
            let (issue, settle, rate, par) = (v[0], v[1], v[2], v[3]);
            if rate <= 0.0 || par <= 0.0 || settle <= issue {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(par * rate * year_fraction(issue, settle, basis))
        }
        "PRICEDISC" => {
            let (settle, mature, discount, redemption) = (v[0], v[1], v[2], v[3]);
            if discount <= 0.0 || redemption <= 0.0 || mature <= settle {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number(redemption - discount * redemption * year_fraction(settle, mature, basis))
        }
        "YIELDDISC" => {
            let (settle, mature, price, redemption) = (v[0], v[1], v[2], v[3]);
            let frac = year_fraction(settle, mature, basis);
            if price <= 0.0 || redemption <= 0.0 || mature <= settle || frac <= 0.0 {
                return Value::Error(ErrorValue::Num);
            }
            Value::Number((redemption / price - 1.0) / frac)
        }
        "PRICEMAT" | "YIELDMAT" => {
            let (settle, mature, issue, rate, fourth) = (v[0], v[1], v[2], v[3], v[4]);
            if rate < 0.0 || mature <= settle || settle <= issue {
                return Value::Error(ErrorValue::Num);
            }
            // Interest accrues from *issue*, not from settlement: the buyer
            // pays the seller for the part of the term already elapsed.
            let fim = year_fraction(issue, mature, basis);
            let fsm = year_fraction(settle, mature, basis);
            let fis = year_fraction(issue, settle, basis);
            if name == "PRICEMAT" {
                let denom = 1.0 + fsm * fourth;
                if denom == 0.0 {
                    return Value::Error(ErrorValue::Num);
                }
                Value::Number((100.0 + fim * rate * 100.0) / denom - fis * rate * 100.0)
            } else {
                let price = fourth;
                if price <= 0.0 || fsm <= 0.0 {
                    return Value::Error(ErrorValue::Num);
                }
                Value::Number(
                    ((100.0 + fim * rate * 100.0) / (price + fis * rate * 100.0) - 1.0) / fsm,
                )
            }
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// The `.INTL` variants of NETWORKDAYS and WORKDAY, where the caller says which
/// days are the weekend.
///
/// `weekend` is either one of Excel's numbered presets or a seven-character
/// mask starting on **Monday** — `"0000011"` is Saturday and Sunday. The mask
/// starting on Monday while `WEEKDAY` counts from Sunday is the trap: reading
/// the mask with a Sunday origin shifts every weekend by a day.
fn eval_workdays_intl(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], advance: bool) -> Value {
    if args.len() < 2 || args.len() > 4 {
        return Value::Error(ErrorValue::Value);
    }
    let (start, second) = match pair_of_numbers(ev, sheet, &args[..2]) {
        Ok([a, b]) => (a.trunc() as i64, b.trunc()),
        Err(e) => return e,
    };

    // Monday-origin mask, `true` where the day is a weekend.
    let mut mask = [false, false, false, false, false, true, true];
    if let Some(arg) = args.get(2) {
        let value = ev.eval_expr(sheet, arg);
        match &value {
            Value::Text(s) if s.len() == 7 && s.bytes().all(|b| b == b'0' || b == b'1') => {
                for (i, b) in s.bytes().enumerate() {
                    mask[i] = b == b'1';
                }
                if mask.iter().all(|d| *d) {
                    // Every day a weekend never terminates in WORKDAY and
                    // counts nothing in NETWORKDAYS; Excel rejects it.
                    return Value::Error(ErrorValue::Value);
                }
            }
            Value::Number(_) | Value::Bool(_) | Value::Empty => {
                let code = value.as_number().unwrap_or(1.0) as i64;
                // 1..=7 are the two-day weekends starting Sat/Sun, 11..=17 the
                // single-day ones starting Sunday.
                mask = [false; 7];
                match code {
                    1..=7 => {
                        // Preset 1 is Sat+Sun; each step moves the pair on by a
                        // day. In Monday-origin indices, 1 → {5,6}.
                        let first = (code + 3).rem_euclid(7) as usize;
                        mask[first] = true;
                        mask[(first + 1) % 7] = true;
                    }
                    11..=17 => {
                        // 11 is Sunday only, which is index 6 Monday-origin.
                        mask[((code - 11 + 6) % 7) as usize] = true;
                    }
                    _ => return Value::Error(ErrorValue::Num),
                }
            }
            Value::Error(e) => return Value::Error(*e),
            _ => return Value::Error(ErrorValue::Value),
        }
    }

    let holidays: Vec<i64> = match args.get(3) {
        Some(_) => match flatten_numbers(ev, sheet, &args[3..]) {
            Ok(ns) => ns.into_iter().map(|n| n.trunc() as i64).collect(),
            Err(e) => return Value::Error(e),
        },
        None => Vec::new(),
    };
    let is_workday = |serial: i64| {
        // `weekday_of` is Sunday-origin (0 = Sunday); the mask is Monday-origin.
        let monday_origin = (weekday_of(serial) + 6) % 7;
        !mask[monday_origin as usize] && !holidays.contains(&serial)
    };

    if advance {
        let mut remaining = second as i64;
        if remaining == 0 {
            return Value::Number(start as f64);
        }
        let step = if remaining > 0 { 1 } else { -1 };
        let mut at = start;
        let mut guard = 0;
        while remaining != 0 && guard < 4_000_000 {
            at += step;
            if is_workday(at) {
                remaining -= step;
            }
            guard += 1;
        }
        return Value::Number(at as f64);
    }
    let end = second as i64;
    let (lo, hi) = (start.min(end), start.max(end));
    let count = (lo..=hi).filter(|d| is_workday(*d)).count() as f64;
    Value::Number(if end < start { -count } else { count })
}

/// `DATEVALUE` / `TIMEVALUE` — text to a serial.
///
/// Only the unambiguous forms are accepted. `03/04/2024` is 3 April in most of
/// the world and 4 March in the United States, and there is no locale here to
/// decide; guessing would silently produce the wrong date a third of the time,
/// so it is `#VALUE!` instead.
fn eval_datevalue(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], time: bool) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]) {
        Value::Error(e) => return Value::Error(e),
        v => v.as_text().unwrap_or_default(),
    };
    let text = text.trim();
    if time {
        return match parse_time_text(text) {
            Some(f) => Value::Number(f),
            None => Value::Error(ErrorValue::Value),
        };
    }
    match parse_date_text(text) {
        // A date carries no time of day, so the serial is whole — DATEVALUE
        // discards any time in the text, as Excel does.
        Some(serial) => Value::Number(serial as f64),
        None => Value::Error(ErrorValue::Value),
    }
}

/// `YYYY-MM-DD` (ISO) or `D-MMM-YYYY` / `MMM D, YYYY`, which name their month.
fn parse_date_text(text: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let head = text.split_whitespace().next().unwrap_or(text);
    let iso: Vec<&str> = head.split('-').collect();
    if iso.len() == 3
        && iso[0].len() == 4
        && let (Ok(y), Ok(m), Ok(d)) = (
            iso[0].parse::<i64>(),
            iso[1].parse::<i64>(),
            iso[2].parse::<i64>(),
        )
    {
        return valid_ymd(y, m, d);
    }
    // Named-month forms, in either order, with any of space, `-` or `,`.
    let parts: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c == '-' || c == ',')
        .filter(|p| !p.is_empty())
        .map(|p| p.to_ascii_lowercase())
        .collect();
    if parts.len() != 3 {
        return None;
    }
    let month = parts
        .iter()
        .position(|p| MONTHS.iter().any(|m| p.starts_with(m)))?;
    let m = MONTHS.iter().position(|m| parts[month].starts_with(m))? as i64 + 1;
    let others: Vec<i64> = parts
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != month)
        .map(|(_, p)| p.parse::<i64>().unwrap_or(-1))
        .collect();
    if others.iter().any(|n| *n < 0) {
        return None;
    }
    // Whichever number cannot be a day is the year; otherwise the year is the
    // one after the month, as in "May 17, 2024".
    let (y, d) = if others[0] > 31 {
        (others[0], others[1])
    } else {
        (others[1], others[0])
    };
    valid_ymd(if y < 100 { y + 2000 } else { y }, m, d)
}

/// A serial, or `None` when the date does not exist — 31 February included.
fn valid_ymd(y: i64, m: i64, d: i64) -> Option<i64> {
    if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) || !(1900..=9999).contains(&y) {
        return None;
    }
    Some(ymd_to_serial(y, m, d))
}

/// `h:mm[:ss] [AM|PM]` as a fraction of a day.
fn parse_time_text(text: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    let pm = lower.contains("pm");
    let am = lower.contains("am");
    let body = lower.replace("am", "").replace("pm", "");
    let parts: Vec<&str> = body.trim().split(':').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let h: f64 = parts[0].trim().parse().ok()?;
    let m: f64 = parts.get(1).map_or(Ok(0.0), |p| p.trim().parse()).ok()?;
    let s: f64 = parts.get(2).map_or(Ok(0.0), |p| p.trim().parse()).ok()?;
    if !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&s) {
        return None;
    }
    // 12 AM is midnight and 12 PM is noon — the one case where adding 12 hours
    // for PM gives the wrong answer.
    let hour = if am || pm {
        if !(1.0..=12.0).contains(&h) {
            return None;
        }
        match (pm, h) {
            (true, 12.0) => 12.0,
            (true, _) => h + 12.0,
            (false, 12.0) => 0.0,
            (false, _) => h,
        }
    } else {
        if !(0.0..=24.0).contains(&h) {
            return None;
        }
        h
    };
    Some((hour * 3600.0 + m * 60.0 + s) / 86400.0)
}

/// The volatile functions: the clock and the random generator.
///
/// Both read state the host supplies rather than the machine's, which is what
/// keeps a recalculation reproducible — see `Workbook::volatile_now`.
fn eval_volatile(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    match name {
        "TODAY" | "NOW" => {
            if !args.is_empty() {
                return Value::Error(ErrorValue::Value);
            }
            let now = ev.now_serial();
            Value::Number(if name == "TODAY" { now.floor() } else { now })
        }
        "RAND" => {
            if !args.is_empty() {
                return Value::Error(ErrorValue::Value);
            }
            Value::Number(ev.next_random())
        }
        "RANDBETWEEN" => {
            let [lo, hi] = match pair_of_numbers(ev, sheet, args) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let (lo, hi) = (lo.ceil(), hi.floor());
            if hi < lo {
                return Value::Error(ErrorValue::Num);
            }
            let span = hi - lo + 1.0;
            // Both ends inclusive, so the draw is scaled across the whole span
            // and floored — clamping guards the one-in-2^53 case where the
            // draw rounds up to exactly 1.
            Value::Number((lo + (ev.next_random() * span).floor()).min(hi))
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// Whether a character occupies two bytes in a double-byte character set.
///
/// The `*B` text functions count bytes, not characters, and in a DBCS locale a
/// full-width character is two. Aliasing them to their character versions —
/// which is what they collapse to in a single-byte locale — would silently
/// halve every count on Japanese or Chinese text, which is precisely the data
/// they exist for.
///
/// The ranges are the full-width and CJK blocks: CJK ideographs, kana, Hangul,
/// and the full-width forms of the ASCII punctuation.
fn is_double_byte(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x115F      // Hangul Jamo
        | 0x2E80..=0x303E    // CJK radicals, kangxi, CJK symbols
        | 0x3041..=0x33FF    // kana, Hangul compat, CJK compat
        | 0x3400..=0x4DBF    // CJK extension A
        | 0x4E00..=0x9FFF    // CJK unified ideographs
        | 0xA000..=0xA4CF    // Yi
        | 0xAC00..=0xD7A3    // Hangul syllables
        | 0xF900..=0xFAFF    // CJK compatibility ideographs
        | 0xFE30..=0xFE6F    // CJK compatibility forms
        | 0xFF00..=0xFF60    // full-width forms
        | 0xFFE0..=0xFFE6    // full-width signs
        | 0x1F300..=0x1F64F  // emoji, which Excel also counts as wide
        | 0x20000..=0x2FA1F  // CJK extensions B..F
    )
}

/// The byte width of a string under DBCS rules.
fn dbcs_len(text: &str) -> usize {
    text.chars()
        .map(|c| if is_double_byte(c) { 2 } else { 1 })
        .sum()
}

/// Take characters until `bytes` byte-widths are used.
///
/// A cut that would land inside a double-byte character stops before it: Excel
/// pads with a space in that case, and half of a character is not a character.
fn dbcs_take(text: &str, bytes: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in text.chars() {
        let w = if is_double_byte(c) { 2 } else { 1 };
        if used + w > bytes {
            // Landing mid-character: Excel emits a space for the half it can
            // not represent, so the result still has the requested width.
            if used < bytes {
                out.push(' ');
            }
            break;
        }
        out.push(c);
        used += w;
    }
    out
}

/// The character index at or after a byte offset, for the `*B` functions that
/// take a start position.
fn dbcs_char_index(text: &str, byte_pos: usize) -> usize {
    let mut used = 0usize;
    for (i, c) in text.chars().enumerate() {
        if used >= byte_pos {
            return i;
        }
        used += if is_double_byte(c) { 2 } else { 1 };
    }
    text.chars().count()
}

/// The byte-oriented text functions.
///
/// Each is its character-counting twin measured in DBCS bytes. On text with no
/// double-byte characters they agree exactly, which is what makes them safe to
/// use in a single-byte locale — and why a test asserts both halves.
fn eval_text_bytes(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    let text_arg = |ev: &mut Evaluator<'_>, i: usize| -> Result<String, ErrorValue> {
        ev.eval_expr(sheet, &args[i]).as_text()
    };
    match name {
        "LENB" => {
            if args.len() != 1 {
                return Value::Error(ErrorValue::Value);
            }
            match text_arg(ev, 0) {
                Ok(t) => Value::Number(dbcs_len(&t) as f64),
                Err(e) => Value::Error(e),
            }
        }
        "LEFTB" | "RIGHTB" => match text_and_count(ev, sheet, args) {
            Ok((text, count)) => {
                let count = count as usize;
                if name == "LEFTB" {
                    Value::Text(dbcs_take(&text, count))
                } else {
                    // From the right: drop the leading bytes instead.
                    let total = dbcs_len(&text);
                    let skip = total.saturating_sub(count);
                    let at = dbcs_char_index(&text, skip);
                    Value::Text(text.chars().skip(at).collect())
                }
            }
            Err(e) => Value::Error(e),
        },
        "MIDB" => {
            if args.len() != 3 {
                return Value::Error(ErrorValue::Value);
            }
            let text = match text_arg(ev, 0) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            let [start, count] = match pair_of_numbers(ev, sheet, &args[1..3]) {
                Ok(v) => v,
                Err(e) => return e,
            };
            if start < 1.0 || count < 0.0 {
                return Value::Error(ErrorValue::Value);
            }
            let at = dbcs_char_index(&text, start as usize - 1);
            let rest: String = text.chars().skip(at).collect();
            Value::Text(dbcs_take(&rest, count as usize))
        }
        "FINDB" | "SEARCHB" => {
            if args.len() < 2 || args.len() > 3 {
                return Value::Error(ErrorValue::Value);
            }
            let needle = match text_arg(ev, 0) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            let hay = match text_arg(ev, 1) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            let start = match args.get(2) {
                Some(a) => match ev.eval_expr(sheet, a).as_number() {
                    Ok(n) => n as usize,
                    Err(e) => return Value::Error(e),
                },
                None => 1,
            };
            if start < 1 {
                return Value::Error(ErrorValue::Value);
            }
            let from_char = dbcs_char_index(&hay, start - 1);
            let rest: String = hay.chars().skip(from_char).collect();
            // FINDB is case-sensitive; SEARCHB is not — the same split as
            // between FIND and SEARCH.
            let found = if name == "FINDB" {
                rest.find(&needle)
            } else {
                rest.to_lowercase().find(&needle.to_lowercase())
            };
            match found {
                Some(byte_off) => {
                    // `find` gives a UTF-8 offset; convert through characters to
                    // a DBCS byte position, which is a different measure again.
                    let chars_before = rest[..byte_off].chars().count();
                    let prefix: String = hay.chars().take(from_char + chars_before).collect();
                    Value::Number(dbcs_len(&prefix) as f64 + 1.0)
                }
                None => Value::Error(ErrorValue::Value),
            }
        }
        "REPLACEB" => {
            if args.len() != 4 {
                return Value::Error(ErrorValue::Value);
            }
            let text = match text_arg(ev, 0) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            let [start, count] = match pair_of_numbers(ev, sheet, &args[1..3]) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let with = match text_arg(ev, 3) {
                Ok(t) => t,
                Err(e) => return Value::Error(e),
            };
            if start < 1.0 || count < 0.0 {
                return Value::Error(ErrorValue::Value);
            }
            let head_chars = dbcs_char_index(&text, start as usize - 1);
            let head: String = text.chars().take(head_chars).collect();
            let tail_at = dbcs_char_index(&text, start as usize - 1 + count as usize);
            let tail: String = text.chars().skip(tail_at).collect();
            Value::Text(format!("{head}{with}{tail}"))
        }
        _ => Value::Error(ErrorValue::Name),
    }
}

/// `ASC` and `JIS` — full-width ↔ half-width conversion.
///
/// Only the ASCII range and the katakana that have both forms convert; anything
/// else passes through, which is what Excel does and what stops the function
/// mangling text that has no half-width equivalent.
fn eval_width_convert(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], to_full: bool) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    let text = match ev.eval_expr(sheet, &args[0]).as_text() {
        Ok(t) => t,
        Err(e) => return Value::Error(e),
    };
    let converted: String = text
        .chars()
        .map(|c| {
            let code = c as u32;
            if to_full {
                match code {
                    // ASCII printable → its full-width twin.
                    0x21..=0x7E => char::from_u32(code + 0xFEE0).unwrap_or(c),
                    0x20 => '\u{3000}', // the ideographic space
                    _ => c,
                }
            } else {
                match code {
                    0xFF01..=0xFF5E => char::from_u32(code - 0xFEE0).unwrap_or(c),
                    0x3000 => ' ',
                    _ => c,
                }
            }
        })
        .collect();
    Value::Text(converted)
}

/// `BAHTTEXT` — a number as Thai baht in words.
///
/// Thai number words are positional with two irregularities that make a naive
/// digit-by-digit rendering wrong: a tens digit of 1 is `สิบ` rather than
/// `หนึ่งสิบ`, and a units digit of 1 after any tens is `เอ็ด` rather than
/// `หนึ่ง`. Both are the difference between correct Thai and something that
/// reads as a foreigner's guess.
fn thai_number(mut n: u64) -> String {
    const DIGITS: [&str; 10] = [
        "",
        "หนึ่ง",
        "สอง",
        "สาม",
        "สี่",
        "ห้า",
        "หก",
        "เจ็ด",
        "แปด",
        "เก้า",
    ];
    const PLACES: [&str; 6] = ["", "สิบ", "ร้อย", "พัน", "หมื่น", "แสน"];
    if n == 0 {
        return "ศูนย์".to_owned();
    }
    let mut out = String::new();
    // Above a million the whole millions part is spoken then suffixed, which
    // recurses rather than needing place names beyond แสน.
    if n >= 1_000_000 {
        out.push_str(&thai_number(n / 1_000_000));
        out.push_str("ล้าน");
        n %= 1_000_000;
        if n == 0 {
            return out;
        }
    }
    let digits: Vec<u32> = n
        .to_string()
        .chars()
        .filter_map(|c| c.to_digit(10))
        .collect();
    let len = digits.len();
    for (i, d) in digits.iter().enumerate() {
        if *d == 0 {
            continue;
        }
        let place = len - 1 - i;
        if place == 1 && *d == 1 {
            out.push_str(PLACES[1]); // สิบ, not หนึ่งสิบ
        } else if place == 1 && *d == 2 {
            out.push_str("ยี่");
            out.push_str(PLACES[1]);
        } else if place == 0 && *d == 1 && len > 1 {
            out.push_str("เอ็ด"); // the special unit after any tens
        } else {
            out.push_str(DIGITS[*d as usize]);
            out.push_str(PLACES[place]);
        }
    }
    out
}

fn eval_bahttext(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() != 1 {
        return Value::Error(ErrorValue::Value);
    }
    let n = match ev.eval_expr(sheet, &args[0]).as_number() {
        Ok(v) => v,
        Err(e) => return Value::Error(e),
    };
    let negative = n < 0.0;
    let abs = n.abs();
    let baht = abs.trunc() as u64;
    // Satang are hundredths, rounded — a half-satang has nowhere to go.
    let satang = ((abs - abs.trunc()) * 100.0).round() as u64;
    let mut out = String::new();
    if negative {
        out.push_str("ลบ");
    }
    out.push_str(&thai_number(baht));
    out.push_str("บาท");
    if satang == 0 {
        out.push_str("ถ้วน"); // "exactly", which Thai requires when there is no change
    } else {
        out.push_str(&thai_number(satang));
        out.push_str("สตางค์");
    }
    Value::Text(out)
}

/// The Bessel functions of the first and second kind, and their modified forms.
///
/// Series expansions rather than a table: the series converge quickly for the
/// arguments a spreadsheet sees, and a table would be an approximation with an
/// arbitrary cut-off rather than one with a stated error.
fn bessel_j(n: i32, x: f64) -> f64 {
    // Ascending series: sum (-1)^k (x/2)^(2k+n) / (k! (k+n)!).
    let mut term = (x / 2.0).powi(n) / factorial_f64(n as u32);
    let mut sum = term;
    let half_sq = (x / 2.0) * (x / 2.0);
    for k in 1..200 {
        term *= -half_sq / (k as f64 * (k + n) as f64);
        sum += term;
        if term.abs() < 1e-18 * sum.abs().max(1e-300) {
            break;
        }
    }
    sum
}

fn bessel_i(n: i32, x: f64) -> f64 {
    // The modified form is the same series without the alternating sign.
    let mut term = (x / 2.0).powi(n) / factorial_f64(n as u32);
    let mut sum = term;
    let half_sq = (x / 2.0) * (x / 2.0);
    for k in 1..300 {
        term *= half_sq / (k as f64 * (k + n) as f64);
        sum += term;
        if term.abs() < 1e-18 * sum.abs().max(1e-300) {
            break;
        }
    }
    sum
}

/// `n!` as a float, for the Bessel series. Named apart from the spreadsheet
/// `FACT`, which returns a `Value` and validates its domain.
fn factorial_f64(n: u32) -> f64 {
    (1..=n).map(f64::from).product::<f64>().max(1.0)
}

/// `BESSELY` and `BESSELK` — the second-kind pair, built from the first.
///
/// Both diverge at zero and are undefined for negative arguments, which is a
/// `#NUM!` rather than an infinity: a spreadsheet showing `1E+308` for an
/// undefined value is worse than one that says so.
fn bessel_y(n: i32, x: f64) -> f64 {
    // Y_n via the limit form, using the recurrence from Y0 and Y1 computed by
    // their standard series with the Euler–Mascheroni term.
    const EULER: f64 = 0.577_215_664_901_532_9;
    let y0 = {
        let mut sum = 0.0;
        let mut term = 1.0;
        let half_sq = (x / 2.0) * (x / 2.0);
        let mut harmonic = 0.0;
        for k in 1..200 {
            term *= -half_sq / (k as f64 * k as f64);
            harmonic += 1.0 / k as f64;
            sum += term * harmonic;
            if term.abs() < 1e-18 {
                break;
            }
        }
        2.0 / std::f64::consts::PI * ((x / 2.0).ln() + EULER) * bessel_j(0, x)
            - 2.0 / std::f64::consts::PI * sum
    };
    if n == 0 {
        return y0;
    }
    let y1 = 2.0 / std::f64::consts::PI * (bessel_j(1, x) * ((x / 2.0).ln() + EULER) - 1.0 / x)
        - bessel_series_y1_correction(x);
    if n == 1 {
        return y1;
    }
    // Upward recurrence, which is stable for Y.
    let (mut prev, mut cur) = (y0, y1);
    for k in 1..n {
        let next = 2.0 * k as f64 / x * cur - prev;
        prev = cur;
        cur = next;
    }
    cur
}

/// The series part of `Y1` that is not expressible through `J1`.
fn bessel_series_y1_correction(x: f64) -> f64 {
    let half = x / 2.0;
    let half_sq = half * half;
    let mut term = half;
    let mut sum = 0.0;
    let mut h_k = 0.0;
    let mut h_k1 = 1.0;
    for k in 0..200 {
        if k > 0 {
            term *= -half_sq / (k as f64 * (k + 1) as f64);
            h_k += 1.0 / k as f64;
            h_k1 += 1.0 / (k + 1) as f64;
        }
        sum += term * (h_k + h_k1);
        if term.abs() < 1e-18 {
            break;
        }
    }
    sum / std::f64::consts::PI
}

fn bessel_k(n: i32, x: f64) -> f64 {
    // K via the integral-free relation to I, using the standard K0/K1 series
    // and upward recurrence.
    const EULER: f64 = 0.577_215_664_901_532_9;
    let k0 = {
        let mut sum = 0.0;
        let mut term = 1.0;
        let half_sq = (x / 2.0) * (x / 2.0);
        let mut harmonic = 0.0;
        for k in 1..300 {
            term *= half_sq / (k as f64 * k as f64);
            harmonic += 1.0 / k as f64;
            sum += term * harmonic;
            if term.abs() < 1e-18 {
                break;
            }
        }
        -((x / 2.0).ln() + EULER) * bessel_i(0, x) + sum
    };
    if n == 0 {
        return k0;
    }
    let k1 = (1.0 / x) * (1.0 - x * k0 * 0.0) + {
        // K1 = 1/x + ln(x/2)·I1 − series; assembled from the same pieces.
        let half = x / 2.0;
        let half_sq = half * half;
        let mut term = half;
        let mut sum = 0.0;
        let mut h_k = 0.0;
        let mut h_k1 = 1.0;
        for k in 0..300 {
            if k > 0 {
                term *= half_sq / (k as f64 * (k + 1) as f64);
                h_k += 1.0 / k as f64;
                h_k1 += 1.0 / (k + 1) as f64;
            }
            sum += term * (h_k + h_k1);
            if term.abs() < 1e-18 {
                break;
            }
        }
        ((x / 2.0).ln() + EULER) * bessel_i(1, x) - sum / 2.0
    };
    if n == 1 {
        return k1;
    }
    let (mut prev, mut cur) = (k0, k1);
    for k in 1..n {
        let next = 2.0 * k as f64 / x * cur + prev;
        prev = cur;
        cur = next;
    }
    cur
}

fn eval_bessel(ev: &mut Evaluator<'_>, sheet: usize, name: &str, args: &[Expr]) -> Value {
    let [x, order] = match pair_of_numbers(ev, sheet, args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let n = order.trunc() as i32;
    if n < 0 || x < 0.0 {
        return Value::Error(ErrorValue::Num);
    }
    // Y and K diverge at zero; J and I are defined there.
    if x == 0.0 && matches!(name, "BESSELY" | "BESSELK") {
        return Value::Error(ErrorValue::Num);
    }
    let v = match name {
        "BESSELJ" => bessel_j(n, x),
        "BESSELI" => bessel_i(n, x),
        "BESSELY" => bessel_y(n, x),
        _ => bessel_k(n, x),
    };
    if v.is_finite() {
        Value::Number(v)
    } else {
        Value::Error(ErrorValue::Num)
    }
}

/// `VDB` — declining-balance depreciation over an arbitrary span of periods,
/// switching to straight line once that gives more.
///
/// The switch is the whole point of the function and the thing `DDB` lacks:
/// declining balance never reaches the salvage value, so an asset depreciated
/// purely that way is still on the books at the end of its life. `no_switch`
/// turns it off for the jurisdictions that require pure declining balance.
///
/// Partial periods are handled by prorating the first and last, which is why
/// `start_period` and `end_period` are floats rather than counts.
fn eval_vdb(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 5 || args.len() > 7 {
        return Value::Error(ErrorValue::Value);
    }
    let v = match opt_numbers(ev, sheet, args, 5, [0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (cost, salvage, life, start, end, factor) = (v[0], v[1], v[2], v[3], v[4], v[5]);
    let no_switch = v[6] != 0.0;
    if cost < 0.0 || salvage < 0.0 || life <= 0.0 || start < 0.0 || end < start || factor <= 0.0 {
        return Value::Error(ErrorValue::Num);
    }

    // Depreciation for one whole period index, given what is already written
    // off. Straight line is measured over the periods *remaining*, which is
    // what makes the two curves cross rather than run parallel.
    let period_amount = |index: f64, accumulated: f64| -> f64 {
        let book = cost - accumulated;
        let declining = (book * factor / life).min(book - salvage).max(0.0);
        if no_switch {
            return declining;
        }
        let remaining = life - index;
        let straight = if remaining > 0.0 {
            ((book - salvage) / remaining).max(0.0)
        } else {
            (book - salvage).max(0.0)
        };
        declining.max(straight).min((book - salvage).max(0.0))
    };

    // Walk whole periods, accumulating, and take the fraction of the first and
    // last that the requested span actually covers.
    let mut accumulated = 0.0;
    let mut total = 0.0;
    let last = end.ceil() as i64;
    for i in 0..last.max(0) {
        let idx = i as f64;
        let amount = period_amount(idx, accumulated);
        // How much of this period lies inside [start, end].
        let overlap = (end.min(idx + 1.0) - start.max(idx)).clamp(0.0, 1.0);
        total += amount * overlap;
        accumulated += amount;
    }
    Value::Number(total)
}

/// `ACCRINT` — interest accrued on a security that pays periodically.
///
/// `calc_method` decides where accrual starts: `TRUE` (the default) from
/// **issue**, `FALSE` from the first interest date. The difference matters
/// exactly when settlement is past the first coupon, which is when anyone
/// bothers to pass the argument.
fn eval_accrint(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr]) -> Value {
    if args.len() < 6 || args.len() > 8 {
        return Value::Error(ErrorValue::Value);
    }
    let v = match opt_numbers(ev, sheet, args, 6, [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (issue, first, settle, rate, par, freq) = (v[0], v[1], v[2], v[3], v[4], v[5]);
    let basis = v[6] as i64;
    let from_issue = v[7] != 0.0;
    if rate <= 0.0 || par <= 0.0 || !matches!(freq as i64, 1 | 2 | 4) || !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    let start = if from_issue { issue } else { first.max(issue) };
    if settle <= start {
        return Value::Error(ErrorValue::Num);
    }
    Value::Number(par * rate * year_fraction(start, settle, basis))
}

/// `AMORLINC` and `AMORDEGRC` — the French depreciation systems.
///
/// Both prorate the first period from the purchase date to the end of the first
/// accounting period, which is why they take dates where the other
/// depreciation functions take counts. `AMORDEGRC` additionally applies a
/// coefficient set by the asset's life, and forces 50% then 100% in the last
/// two periods — a rule of the tax code rather than of arithmetic, which is
/// why it cannot be derived and has to be written down.
fn eval_amor(ev: &mut Evaluator<'_>, sheet: usize, args: &[Expr], degressive: bool) -> Value {
    if args.len() < 6 || args.len() > 7 {
        return Value::Error(ErrorValue::Value);
    }
    let v = match opt_numbers(ev, sheet, args, 6, [0.0; 7]) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (cost, purchased, first_period, salvage, period, rate) =
        (v[0], v[1], v[2], v[3], v[4], v[5]);
    let basis = v[6] as i64;
    if cost <= 0.0 || rate <= 0.0 || salvage < 0.0 || period < 0.0 || !(0..=4).contains(&basis) {
        return Value::Error(ErrorValue::Num);
    }
    let life = 1.0 / rate;
    let coefficient = if !degressive {
        1.0
    } else {
        // The coefficients are fixed by the life in years, not computed.
        match life {
            l if l < 3.0 => 1.0,
            l if l <= 4.0 => 1.5,
            l if l <= 5.0 => 2.0,
            _ => 2.5,
        }
    };
    let effective_rate = rate * coefficient;
    // The first period runs from purchase to the end of the first accounting
    // period, so it is a fraction of a year rather than a whole one.
    let first_fraction = year_fraction(purchased, first_period, basis);
    let mut book = cost;
    let mut amount = (cost * effective_rate * first_fraction).round();
    if period == 0.0 {
        return Value::Number(amount.min(cost - salvage).max(0.0));
    }
    book -= amount;
    for p in 1..=(period as i64) {
        let remaining_life = life - first_fraction - (p - 1) as f64;
        amount = if !degressive {
            // AMORLINC is *linear*: every full period writes off the same
            // `cost × rate`. Applying the rate to the declining book instead
            // makes it degressive, which is the other function.
            cost * rate
        } else if remaining_life <= 2.0 {
            // The last two periods are forced: half, then whatever is left. A
            // rule of the tax code rather than of arithmetic.
            if remaining_life <= 1.0 {
                book - salvage
            } else {
                (book - salvage) / 2.0
            }
        } else {
            book * effective_rate
        };
        amount = amount.min((book - salvage).max(0.0)).max(0.0);
        if p as f64 == period {
            return Value::Number(amount);
        }
        book -= amount;
    }
    Value::Number(0.0)
}
