#include <algorithm>
#include <cmath>
#include <time.h>
#include <string>
#include <vector>

#define PCRE2_CODE_UNIT_WIDTH 8
#include <pcre2.h>

#include "misc.h"
#include "printout.h"
#include "logger.h"
#include "speedtestutil.h"
#include "rapidjson_extra.h"

using namespace rapidjson;

// =============================================================================
// Protocol parsing has moved to the mihomo kernel (see subconv.cpp). stair no
// longer hand-parses any share-link / config format. What remains here is the
// protocol-agnostic plumbing the rest of the program still needs:
//   * chkIgnore / filterNodes — PCRE2 remark include/exclude filtering
//   * streamToInt / getSubInfoFrom* — subscription traffic/expiry extraction
// =============================================================================

bool chkIgnore(const nodeInfo &node, string_array &exclude_remarks, string_array &include_remarks)
{
    bool excluded = false, included = false;
    std::string remarks = node.remarks;
    excluded = std::any_of(exclude_remarks.cbegin(), exclude_remarks.cend(), [&remarks](const auto &x)
    {
        return regFind(remarks, x);
    });
    if(include_remarks.size() != 0)
    {
        included = std::any_of(include_remarks.cbegin(), include_remarks.cend(), [&remarks](const auto &x)
        {
            return regFind(remarks, x);
        });
    }
    else
    {
        included = true;
    }

    return excluded || !included;
}

void filterNodes(std::vector<nodeInfo> &nodes, string_array &exclude_remarks, string_array &include_remarks, int groupID)
{
    int node_index = 0;
    std::vector<nodeInfo>::iterator iter = nodes.begin();

    std::vector<std::unique_ptr<pcre2_code, decltype(&pcre2_code_free)>> exclude_patterns, include_patterns;
    std::vector<std::unique_ptr<pcre2_match_data, decltype(&pcre2_match_data_free)>> exclude_match_data, include_match_data;
    unsigned int i = 0;
    PCRE2_SIZE erroroffset;
    int errornumber, rc;

    for(i = 0; i < exclude_remarks.size(); i++)
    {
        std::unique_ptr<pcre2_code, decltype(&pcre2_code_free)> pattern(pcre2_compile(reinterpret_cast<const unsigned char*>(exclude_remarks[i].c_str()), exclude_remarks[i].size(), PCRE2_UTF | PCRE2_MULTILINE | PCRE2_ALT_BSUX, &errornumber, &erroroffset, NULL), &pcre2_code_free);
        if(!pattern)
            return;
        exclude_patterns.emplace_back(std::move(pattern));
        pcre2_jit_compile(exclude_patterns[i].get(), 0);
        std::unique_ptr<pcre2_match_data, decltype(&pcre2_match_data_free)> match_data(pcre2_match_data_create_from_pattern(exclude_patterns[i].get(), NULL), &pcre2_match_data_free);
        exclude_match_data.emplace_back(std::move(match_data));
    }
    for(i = 0; i < include_remarks.size(); i++)
    {
        std::unique_ptr<pcre2_code, decltype(&pcre2_code_free)> pattern(pcre2_compile(reinterpret_cast<const unsigned char*>(include_remarks[i].c_str()), include_remarks[i].size(), PCRE2_UTF | PCRE2_MULTILINE | PCRE2_ALT_BSUX, &errornumber, &erroroffset, NULL), &pcre2_code_free);
        if(!pattern)
            return;
        include_patterns.emplace_back(std::move(pattern));
        pcre2_jit_compile(include_patterns[i].get(), 0);
        std::unique_ptr<pcre2_match_data, decltype(&pcre2_match_data_free)> match_data(pcre2_match_data_create_from_pattern(include_patterns[i].get(), NULL), &pcre2_match_data_free);
        include_match_data.emplace_back(std::move(match_data));
    }
    writeLog(LOG_TYPE_INFO, "Filter started.");
    while(iter != nodes.end())
    {
        bool excluded = false, included = false;
        for(i = 0; i < exclude_patterns.size(); i++)
        {
            rc = pcre2_match(exclude_patterns[i].get(), reinterpret_cast<const unsigned char*>(iter->remarks.c_str()), iter->remarks.size(), 0, 0, exclude_match_data[i].get(), NULL);
            if (rc < 0)
            {
                switch(rc)
                {
                case PCRE2_ERROR_NOMATCH: break;
                default: return;
                }
            }
            else
                excluded = true;
        }
        if(include_patterns.size() > 0)
            for(i = 0; i < include_patterns.size(); i++)
            {
                rc = pcre2_match(include_patterns[i].get(), reinterpret_cast<const unsigned char*>(iter->remarks.c_str()), iter->remarks.size(), 0, 0, include_match_data[i].get(), NULL);
                if (rc < 0)
                {
                    switch(rc)
                    {
                    case PCRE2_ERROR_NOMATCH: break;
                    default: return;
                    }
                }
                else
                    included = true;
            }
        else
            included = true;
        if(excluded || !included)
        {
            writeLog(LOG_TYPE_INFO, "Node  " + iter->group + " - " + iter->remarks + "  has been ignored and will not be added.");
            nodes.erase(iter);
        }
        else
        {
            writeLog(LOG_TYPE_INFO, "Node  " + iter->group + " - " + iter->remarks + "  has been added.");
            iter->id = node_index;
            iter->groupID = groupID;
            ++node_index;
            ++iter;
        }
    }
    writeLog(LOG_TYPE_INFO, "Filter done.");
}

unsigned long long streamToInt(const std::string &stream)
{
    if(!stream.size())
        return 0;
    double streamval = 1.0;
    std::vector<std::string> units = {"B", "KB", "MB", "GB", "TB", "PB", "EB"};
    size_t index = units.size();
    do
    {
        index--;
        if(endsWith(stream, units[index]))
        {
            streamval = std::pow(1024, index) * to_number<float>(stream.substr(0, stream.size() - units[index].size()), 0.0);
            break;
        }
    } while(index != 0);
    return (unsigned long long)streamval;
}

static inline double percentToDouble(const std::string &percent)
{
    return stof(percent.substr(0, percent.size() - 1)) / 100.0;
}

time_t dateStringToTimestamp(std::string date)
{
    time_t rawtime;
    time(&rawtime);
    if(startsWith(date, "left="))
    {
        time_t seconds_left = 0;
        date.erase(0, 5);
        if(endsWith(date, "d"))
        {
            date.erase(date.size() - 1);
            seconds_left = to_number<double>(date, 0.0) * 86400.0;
        }
        return rawtime + seconds_left;
    }
    else
    {
        struct tm *expire_time;
        std::vector<std::string> date_array = split(date, ":");
        if(date_array.size() != 6)
            return 0;

        expire_time = localtime(&rawtime);
        expire_time->tm_year = to_int(date_array[0], 1900) - 1900;
        expire_time->tm_mon = to_int(date_array[1], 1) - 1;
        expire_time->tm_mday = to_int(date_array[2]);
        expire_time->tm_hour = to_int(date_array[3]);
        expire_time->tm_min = to_int(date_array[4]);
        expire_time->tm_sec = to_int(date_array[5]);
        return mktime(expire_time);
    }
}

bool getSubInfoFromHeader(const std::string &header, std::string &result)
{
    std::string pattern = R"(^(?i:Subscription-UserInfo): (.*?)\s*?$)", retStr;
    if(regFind(header, pattern))
    {
        regGetMatch(header, pattern, 2, 0, &retStr);
        if(retStr.size())
        {
            result = retStr;
            return true;
        }
    }
    return false;
}

bool getSubInfoFromNodes(const std::vector<nodeInfo> &nodes, const string_array &stream_rules, const string_array &time_rules, std::string &result)
{
    std::string remarks, pattern, target, stream_info, time_info, retStr;
    string_size spos;

    for(const nodeInfo &x : nodes)
    {
        remarks = x.remarks;
        if(!stream_info.size())
        {
            for(const std::string &y : stream_rules)
            {
                spos = y.rfind("|");
                if(spos == y.npos)
                    continue;
                pattern = y.substr(0, spos);
                target = y.substr(spos + 1);
                if(regMatch(remarks, pattern))
                {
                    retStr = regReplace(remarks, pattern, target);
                    if(retStr != remarks)
                    {
                        stream_info = retStr;
                        break;
                    }
                }
                else
                    continue;
            }
        }

        remarks = x.remarks;
        if(!time_info.size())
        {
            for(const std::string &y : time_rules)
            {
                spos = y.rfind("|");
                if(spos == y.npos)
                    continue;
                pattern = y.substr(0, spos);
                target = y.substr(spos + 1);
                if(regMatch(remarks, pattern))
                {
                    retStr = regReplace(remarks, pattern, target);
                    if(retStr != remarks)
                    {
                        time_info = retStr;
                        break;
                    }
                }
                else
                    continue;
            }
        }

        if(stream_info.size() && time_info.size())
            break;
    }

    if(!stream_info.size() && !time_info.size())
        return false;

    unsigned long long total = 0, left, used = 0, expire = 0;
    std::string total_str = getUrlArg(stream_info, "total"), left_str = getUrlArg(stream_info, "left"), used_str = getUrlArg(stream_info, "used");
    if(strFind(total_str, "%"))
    {
        if(used_str.size())
        {
            used = streamToInt(used_str);
            total = used / (1 - percentToDouble(total_str));
        }
        else if(left_str.size())
        {
            left = streamToInt(left_str);
            total = left / percentToDouble(total_str);
            used = total - left;
        }
    }
    else
    {
        total = streamToInt(total_str);
        if(used_str.size())
        {
            used = streamToInt(used_str);
        }
        else if(left_str.size())
        {
            left = streamToInt(left_str);
            used = total - left;
        }
    }

    result = "upload=0; download=" + std::to_string(used) + "; total=" + std::to_string(total) + ";";

    expire = dateStringToTimestamp(time_info);
    if(expire)
        result += " expire=" + std::to_string(expire) + ";";

    return true;
}

bool getSubInfoFromSSD(const std::string &sub, std::string &result)
{
    rapidjson::Document json;
    json.Parse(urlsafe_base64_decode(sub.substr(6)).data());
    if(json.HasParseError())
        return false;

    std::string used_str = GetMember(json, "traffic_used"), total_str = GetMember(json, "traffic_total"), expire_str = GetMember(json, "expiry");
    if(!used_str.size() || !total_str.size())
        return false;
    unsigned long long used = stod(used_str) * std::pow(1024, 3), total = stod(total_str) * std::pow(1024, 3), expire;
    result = "upload=0; download=" + std::to_string(used) + "; total=" + std::to_string(total) + ";";

    expire = dateStringToTimestamp(regReplace(expire_str, "(\\d+)-(\\d+)-(\\d+) (.*)", "$1:$2:$3:$4"));
    if(expire)
        result += " expire=" + std::to_string(expire) + ";";

    return true;
}