static char *get_log_timestamp(void)
{
	struct timeval tv;
	static __thread char tstr[28];
	time_t curr_time;
	struct tm time_info;

	gettimeofday(&tv, NULL);
	curr_time = tv.tv_sec;
	localtime_r(&curr_time, &time_info);
	strftime(tstr, 21, "%Y-%m-%d %H:%M:%S.", &time_info);

	snprintf(tstr + 20, 7, "%06u", (unsigned)tv.tv_usec);
	return tstr;
}
