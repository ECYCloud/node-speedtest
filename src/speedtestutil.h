#ifndef SPEEDTESTUTIL_H_INCLUDED
#define SPEEDTESTUTIL_H_INCLUDED

#include <string>

#include "misc.h"
#include "nodeinfo.h"

// Protocol parsing now lives in the mihomo kernel (see subconv.cpp). Only the
// protocol-agnostic helpers below remain here.
bool chkIgnore(const nodeInfo &node, string_array &exclude_remarks, string_array &include_remarks);
void filterNodes(std::vector<nodeInfo> &nodes, string_array &exclude_remarks, string_array &include_remarks, int groupID);
bool getSubInfoFromHeader(const std::string &header, std::string &result);
bool getSubInfoFromNodes(const std::vector<nodeInfo> &nodes, const string_array &stream_rules, const string_array &time_rules, std::string &result);
bool getSubInfoFromSSD(const std::string &sub, std::string &result);
unsigned long long streamToInt(const std::string &stream);

#endif // SPEEDTESTUTIL_H_INCLUDED
