// https://github.com/vijos/malicious-code
#ifdef _WIN32
	#include <con>
#else
	#include </dev/random>
	#include </dev/urandom>
#endif
