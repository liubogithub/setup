#!/bin/bash
# Liu Bo ftrace switch

ftrace="/sys/kernel/debug/tracing"

do_cleanup()
{
	echo nop > $ftrace/current_tracer
	echo 0 > $ftrace/tracing_on
	echo 0 > $ftrace/function_profile_enabled
	echo > $ftrace/set_ftrace_pid
	echo > $ftrace/trace
	echo > $ftrace/set_graph_function
	echo > $ftrace/set_ftrace_filter
	echo 0 > $ftrace/max_graph_depth
}

do_prep()
{
	# use these for tracer 'function_graph'
	echo 7 > $ftrace/max_graph_depth
	echo btrfs_file_write_iter > $ftrace/set_graph_function
	echo function_graph > $ftrace/current_tracer
	echo funcgraph-tail > $ftrace/trace_options

	# use for tracer 'function'
	#echo nostacktrace > $ftrace/trace_options

	# profile
	: '
	echo 1 > $ftrace/function_profile_enabled
	'

	#set pid 
	#echo $$ > $ftrace/set_ftrace_pid
}

if [ "x$1" = "xon" ]; then
	do_cleanup
	do_prep
	echo 1 > $ftrace/tracing_on

	# customer command
	: '
	rm /mnt/vmdisk/foobar
	dd if=/dev/zero of=/mnt/vmdisk/foobar bs=512 count=10 oflag=direct
	'

elif [ "x$1" = "xoff" ]; then
	echo 0 > $ftrace/tracing_on
	#echo 0 > $ftrace/function_profile_enabled
	echo "now...go check something interesting in trace file......"
else
	echo "invalid args"
	exit
fi

