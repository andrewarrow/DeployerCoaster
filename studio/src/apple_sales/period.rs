use time::{Date, Month};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Frequency {
    Daily,
    #[default]
    Monthly,
}

impl Frequency {
    pub(super) fn api_value(self) -> &'static str {
        match self {
            Self::Daily => "DAILY",
            Self::Monthly => "MONTHLY",
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Monthly => "Monthly",
        }
    }
    pub(super) fn latest(self, today: Date) -> Date {
        match self {
            Self::Daily => today.previous_day().unwrap_or(today),
            Self::Monthly => month_start(today.year(), today.month())
                .previous_day()
                .map(|date| month_start(date.year(), date.month()))
                .unwrap_or(today),
        }
    }
    pub(super) fn previous(self, date: Date) -> Option<Date> {
        match self {
            Self::Daily => date.previous_day(),
            Self::Monthly => {
                let start = month_start(date.year(), date.month());
                let prev = start.previous_day()?;
                Some(month_start(prev.year(), prev.month()))
            }
        }
    }
    pub(super) fn next(self, date: Date, today: Date) -> Option<Date> {
        let candidate = match self {
            Self::Daily => date.next_day()?,
            Self::Monthly => {
                if date.month() == Month::December {
                    Date::from_calendar_date(date.year().checked_add(1)?, Month::January, 1).ok()?
                } else {
                    Date::from_calendar_date(date.year(), date.month().next(), 1).ok()?
                }
            }
        };
        (candidate <= self.latest(today)).then_some(candidate)
    }
    pub(super) fn report_date(self, date: Date) -> String {
        match self {
            Self::Daily => format!(
                "{:04}-{:02}-{:02}",
                date.year(),
                date.month() as u8,
                date.day()
            ),
            Self::Monthly => format!("{:04}-{:02}", date.year(), date.month() as u8),
        }
    }
    pub(super) fn display_date(self, date: Date) -> String {
        match self {
            Self::Daily => format!(
                "{} {}, {}",
                month_abbrev(date.month()),
                date.day(),
                date.year()
            ),
            Self::Monthly => format!("{} {}", month_full(date.month()), date.year()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monthly_boundaries_normalize_and_cross_years() {
        let today = Date::from_calendar_date(2025, Month::January, 12).unwrap();
        let frequency = Frequency::Monthly;
        let latest = frequency.latest(today);
        assert_eq!(
            latest,
            Date::from_calendar_date(2024, Month::December, 1).unwrap()
        );
        assert_eq!(
            frequency.previous(latest),
            Some(Date::from_calendar_date(2024, Month::November, 1).unwrap())
        );
        assert_eq!(frequency.next(latest, today), None);
        assert_eq!(
            frequency.next(frequency.previous(latest).unwrap(), today),
            Some(latest)
        );
        assert_eq!(frequency.report_date(latest), "2024-12");
        assert_eq!(frequency.display_date(latest), "December 2024");
    }

    #[test]
    fn daily_handles_leap_day_and_forward_cutoff() {
        let frequency = Frequency::Daily;
        let today = Date::from_calendar_date(2024, Month::March, 1).unwrap();
        let yesterday = frequency.latest(today);
        assert_eq!(
            yesterday,
            Date::from_calendar_date(2024, Month::February, 29).unwrap()
        );
        assert_eq!(
            frequency.previous(yesterday),
            Some(Date::from_calendar_date(2024, Month::February, 28).unwrap())
        );
        assert_eq!(frequency.next(yesterday, today), None);
        assert_eq!(frequency.report_date(yesterday), "2024-02-29");
        assert_eq!(frequency.display_date(yesterday), "Feb 29, 2024");
    }
}

fn month_start(year: i32, month: Month) -> Date {
    Date::from_calendar_date(year, month, 1).expect("valid calendar month start")
}
fn month_abbrev(month: Month) -> &'static str {
    match month {
        Month::January => "Jan",
        Month::February => "Feb",
        Month::March => "Mar",
        Month::April => "Apr",
        Month::May => "May",
        Month::June => "Jun",
        Month::July => "Jul",
        Month::August => "Aug",
        Month::September => "Sep",
        Month::October => "Oct",
        Month::November => "Nov",
        Month::December => "Dec",
    }
}
fn month_full(month: Month) -> &'static str {
    match month {
        Month::January => "January",
        Month::February => "February",
        Month::March => "March",
        Month::April => "April",
        Month::May => "May",
        Month::June => "June",
        Month::July => "July",
        Month::August => "August",
        Month::September => "September",
        Month::October => "October",
        Month::November => "November",
        Month::December => "December",
    }
}
