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
	echo 0 > $ftrace/tracing_thresh
}

do_prep()
{
	# use these for tracer 'function_graph'
	: '
	echo 3 > $ftrace/max_graph_depth
	echo do_io_submit > $ftrace/set_graph_function
	echo function_graph > $ftrace/current_tracer
	echo funcgraph-tail > $ftrace/trace_options
	'
	# time thresh in function_graph (in microsecond)
	echo 500000 > $ftrace/tracing_thresh
	echo function_graph > $ftrace/current_tracer
	echo funcgraph-tail > $ftrace/trace_options
	echo funcgraph-proc > $ftrace/trace_options

	# use for tracer 'function'
	: '
	echo btrfs_readpages > $ftrace/set_ftrace_filter
	echo function > $ftrace/current_tracer
	echo func_stack_trace > $ftrace/trace_options
	'

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
	tfile=/mnt/btrfs/foobar
	fallocate -l 100M $tfile
	dd if=/dev/zero of=$tfile bs=513 count=10 oflag=direct conv=notrunc
	'

elif [ "x$1" = "xoff" ]; then
	echo nop > $ftrace/current_tracer
	#echo 0 > $ftrace/function_profile_enabled
	echo "now...go check something interesting in trace file......"
else
	echo "invalid args"
	exit
fi

